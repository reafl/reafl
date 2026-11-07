//! Saving, loading and the background-task results.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn save_project(&mut self) {
        self.capture_plugin_states();
        self.project.normalize();
        // Serialization and the write are handed to a worker thread; the clone
        // is cheap compared with encoding a large project.
        let path = self.project_path.clone();
        self.status = format!("Saving {path}…");
        self.tasks.save_project(path, self.project.clone());
    }

    pub(crate) fn load_project(&mut self) {
        let path = self.project_path.clone();
        self.status = format!("Loading {path}…");
        self.tasks.load_project(path);
    }

    /// Applies a project that finished loading: resets the selection, pushes it
    /// to the engine, and restores everything the project references.
    pub(crate) fn adopt_project(&mut self, mut project: Project, path: &str) {
        project.normalize();
        self.project = project;
        self.selected_clip = 0;
        self.selected_instrument = 0;
        self.selected_lane = 0;
        self.selected_mixer_channel = 0;
        self.selection = None;
        self.sync_project();
        self.load_audio_clip_samples();
        self.load_instrument_samples();
        self.restore_hosted_plugins();
        self.status = format!("Loaded project from {path}.");
    }

    /// Drains finished background jobs; call once per frame.
    pub(crate) fn poll_tasks(&mut self) {
        while let Some(task) = self.tasks.try_recv() {
            self.apply_task(task);
        }
    }

    /// Applies one finished job to the app state.
    pub(crate) fn apply_task(&mut self, task: tasks::Task) {
        use tasks::{SampleUse, Task};
        match task {
            Task::PluginsScanned { plugins, error } => {
                self.status = format!("Found {} external plug-in(s).", plugins.len());
                self.external_plugins = plugins;
                self.plugin_scan_error = error;
            }
            Task::SamplesScanned { files } => {
                self.status = format!("{} sample(s) found.", files.len());
                self.sample_files = files;
            }
            Task::SampleDecoded {
                path,
                purpose,
                result,
            } => match (result, purpose) {
                (Ok(sample), SampleUse::Instrument(instrument_id)) => {
                    let frames = sample.frames.len();
                    if self.send_audio(AudioCommand::LoadInstrumentSample {
                        instrument_id,
                        sample,
                    }) {
                        self.status = format!("Loaded {frames} frames into the instrument.");
                    }
                }
                (Ok(sample), SampleUse::Clip) => {
                    let frames = sample.frames.len();
                    if self.send_audio(AudioCommand::LoadAudioClip {
                        path: path.clone(),
                        sample,
                    }) {
                        self.status = format!("Loaded {frames} frames for {path}.");
                    }
                }
                (Ok(sample), SampleUse::Preview) => {
                    self.send_audio(AudioCommand::PreviewSample(Box::new(sample)));
                    self.status = format!("Previewing {path}");
                }
                (Err(error), SampleUse::Instrument(_)) => {
                    self.status = format!("Sample load failed: {error}");
                }
                (Err(error), _) => self.status = format!("Could not read {path}: {error}"),
            },
            Task::ProjectLoaded { path, result } => match result {
                Ok(project) => self.adopt_project(*project, &path),
                Err(error) => self.status = error,
            },
            Task::ProjectSaved { path, result } => match result {
                Ok(()) => self.status = format!("Saved project to {path}."),
                Err(error) => self.status = error,
            },
        }
    }

    /// Re-instantiates every VST3/CLAP bundle the project references. Hosted
    /// instances are never serialized themselves, so a project load has to
    /// rebuild them or the channels would come back silent.
    pub(crate) fn restore_hosted_plugins(&mut self) {
        for id in self.hosted.keys().copied().collect::<Vec<_>>() {
            self.unload_host(id);
        }
        let mut jobs = Vec::new();
        for instrument_id in 0..self.project.tracks.len() {
            let instrument = &self.project.tracks[instrument_id];
            if instrument.synth_kind == SynthKind::External && !instrument.plugin_path.is_empty() {
                jobs.push((
                    pulseforge_core::hosted::instrument_host_id(instrument_id),
                    instrument.plugin_path.clone(),
                    true,
                ));
            }
        }
        let track_count = self.project.tracks.len();
        for channel in 0..=track_count {
            let slots = if channel == track_count {
                &self.project.master.inserts
            } else {
                &self.project.tracks[channel].inserts
            };
            for (slot_index, slot) in slots.iter().enumerate() {
                if !slot.enabled || slot.plugin_path.is_empty() {
                    continue;
                }
                jobs.push((
                    pulseforge_core::hosted::effect_host_id(channel, slot_index),
                    slot.plugin_path.clone(),
                    false,
                ));
            }
        }
        for (id, path, instrument) in jobs {
            self.load_host(id, &path, instrument);
        }
    }

    pub(crate) fn load_instrument_samples(&mut self) {
        for instrument_id in 0..self.project.tracks.len() {
            if !self.project.tracks[instrument_id]
                .sample_path
                .is_empty()
            {
                self.load_instrument_sample(instrument_id);
            }
        }
    }
}

impl PulseForgeApp {
    pub(crate) fn query_midi_ports() -> Result<Vec<String>, String> {
        let input = MidiInput::new("PulseForge MIDI Discovery")
            .map_err(|error| format!("Could not enumerate MIDI inputs: {error}"))?;
        input
            .ports()
            .iter()
            .enumerate()
            .map(|(index, port)| {
                input.port_name(port).map_err(|error| {
                    format!("Could not read MIDI input {} name: {error}", index + 1)
                })
            })
            .collect()
    }

    pub(crate) fn connect_midi(&mut self, index: usize) {
        self.midi_connection.take();
        let Some(sender) = self.sender() else {
            self.status = self
                .audio_error
                .clone()
                .unwrap_or_else(|| "Audio output is unavailable.".to_owned());
            return;
        };
        let mut input = match MidiInput::new("PulseForge MIDI Input") {
            Ok(input) => input,
            Err(error) => {
                self.status = format!("Could not initialize MIDI: {error}");
                return;
            }
        };
        input.ignore(Ignore::None);
        let ports = input.ports();
        let Some(port) = ports.get(index) else {
            self.status = "The selected MIDI input is no longer available.".to_owned();
            return;
        };
        let routes = Arc::clone(&self.midi_routes);
        let callback_error = Arc::clone(&self.midi_callback_error);
        match input.connect(
            port,
            "PulseForge MIDI Input",
            move |_timestamp, message, _| {
                if message.len() < 3 {
                    return;
                }
                let status = message[0];
                let kind = status & 0xF0;
                if kind != 0x80 && kind != 0x90 {
                    return;
                }
                let channel = usize::from(status & 0x0F);
                let instrument_id = match routes.lock() {
                    Ok(routes) => routes.get(channel).copied().unwrap_or(0),
                    Err(_) => {
                        if let Ok(mut error) = callback_error.lock() {
                            *error = Some("MIDI route table became unavailable.".to_owned());
                        }
                        return;
                    }
                };
                let event = AudioCommand::MidiNote {
                    instrument_id,
                    note: message[1],
                    velocity: message[2],
                    on: kind == 0x90 && message[2] > 0,
                };
                if let Err(error) = sender.try_send(event)
                    && let Ok(mut callback_error) = callback_error.lock()
                {
                    *callback_error = Some(format!("MIDI event queue failed: {error}"));
                }
            },
            (),
        ) {
            Ok(connection) => {
                self.midi_connection = Some(connection);
                self.selected_midi_port = Some(index);
                self.status = format!(
                    "Connected MIDI input: {}",
                    self.midi_ports.get(index).map_or("Device", String::as_str)
                );
            }
            Err(error) => self.status = format!("Could not connect MIDI input: {error}"),
        }
    }
}
