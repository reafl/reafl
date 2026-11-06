//! Hosting VST3/CLAP instances and their editor windows.

use crate::*;

impl PulseForgeApp {
    /// Bundle metadata for `path`, read once per path.
    pub(crate) fn metadata_for(&mut self, path: &str) -> pulseforge_plugins::PluginMetadata {
        if let Some((cached_path, metadata)) = &self.plugin_metadata
            && cached_path == path
        {
            return metadata.clone();
        }
        let metadata = pulseforge_plugins::read_metadata(path);
        self.plugin_metadata = Some((path.to_owned(), metadata.clone()));
        metadata
    }

    /// Loads a bundle and hands its audio half to the engine. The editor half
    /// stays here, because both formats require their GUI on the UI thread.
    pub(crate) fn load_host(&mut self, id: u64, path: &str, instrument: bool) {
        self.unload_host(id);
        let Some(audio) = self.audio.as_ref() else {
            self.hosted_errors.insert(
                id,
                "No audio device is running, so the plug-in cannot be hosted.".to_owned(),
            );
            return;
        };
        let sample_rate = f64::from(audio.sample_rate);
        // The counters are created here and travel to the audio thread inside
        // the load command, so block times can be attributed to this slot
        // without any locking on the audio path.
        let counters = self.plugin_debug.register(
            id,
            std::path::Path::new(path)
                .file_stem()
                .map_or_else(|| path.to_owned(), |stem| stem.to_string_lossy().into_owned()),
            path,
            if instrument {
                pulseforge_profiling::PluginKind::Instrument
            } else {
                pulseforge_profiling::PluginKind::Effect
            },
            pulseforge_core::hosted::HOST_BLOCK as u32,
            audio.sample_rate,
        );
        match pulseforge_hosting::load(
            path,
            instrument,
            sample_rate,
            pulseforge_core::hosted::HOST_BLOCK,
        ) {
            Ok((mut ui, audio_half)) => {
                let _ = audio.sender.try_send(AudioCommand::LoadHosted {
                    id,
                    audio: Box::new(audio_half),
                    instrument,
                    counters: Some(Arc::clone(&counters)),
                });
                // Restore the state saved with the project, if any.
                if let Some(state) = self.stored_plugin_state(id)
                    && !state.is_empty()
                {
                    ui.load_state(&state);
                }
                let meta = HostedMeta {
                    info: ui.info().clone(),
                    parameters: ui.parameters(),
                    editor_size: ui.editor_size().unwrap_or((640, 420)),
                };
                self.hosted_meta.insert(id, meta);
                self.hosted.insert(id, ui);
            }
            Err(error) => {
                // Loading failures are the most common "why is my plug-in
                // silent" cause, so they are also kept for the debug panel.
                counters.record_error(&error.to_string());
                self.hosted_errors.insert(id, error.to_string());
            }
        }
    }

    /// Plug-in state stored in the project for a host id, if any.
    pub(crate) fn stored_plugin_state(&self, id: u64) -> Option<Vec<u8>> {
        for (instrument_id, instrument) in self.project.instruments.iter().enumerate() {
            if pulseforge_core::hosted::instrument_host_id(instrument_id) == id {
                return Some(instrument.plugin_state.clone());
            }
        }
        let lane_count = self.project.lanes.len();
        for channel in 0..=lane_count {
            let slots = if channel == lane_count {
                &self.project.master.inserts
            } else {
                &self.project.lanes[channel].inserts
            };
            for (slot, effect) in slots.iter().enumerate() {
                if pulseforge_core::hosted::effect_host_id(channel, slot) == id {
                    return Some(effect.plugin_state.clone());
                }
            }
        }
        None
    }

    pub(crate) fn unload_host(&mut self, id: u64) {
        if self.attached_editor == Some(id) {
            self.detach_editor();
        }
        self.plugin_debug.unregister(id);
        if self.hosted.remove(&id).is_some()
            && let Some(audio) = self.audio.as_ref()
        {
            let _ = audio.sender.try_send(AudioCommand::UnloadHosted { id });
        }
        self.editor_windows.remove(&id);
        self.hosted_errors.remove(&id);
        self.hosted_meta.remove(&id);
    }

    /// Mirrors the plug-in's own editor changes into the "last tweaked"
    /// parameter, which the wrapper offers to automate.
    pub(crate) fn poll_last_tweaked(&mut self) {
        let Some(host_id) = self.attached_editor else {
            return;
        };
        let Some((parameter_id, _value)) = self
            .hosted
            .get_mut(&host_id)
            .and_then(pulseforge_hosting::HostedUi::take_last_tweaked)
        else {
            return;
        };
        let name = self
            .hosted_meta
            .get(&host_id)
            .and_then(|meta| {
                meta.parameters
                    .iter()
                    .find(|parameter| parameter.id == parameter_id)
            })
            .map(|parameter| parameter.name.clone())
            .unwrap_or_else(|| format!("Parameter {parameter_id}"));
        self.last_tweaked = Some((host_id, name));
    }
}

impl PulseForgeApp {
    /// Keeps a plug-in editor in step with the window rectangle it occupies.
    pub(crate) fn resize_editor_to(&mut self, host_id: u64, rect: egui::Rect) {
        if self.attached_editor != Some(host_id) {
            return;
        }
        let scale = self.last_pixels_per_point.max(0.5);
        let wanted = (
            (rect.width() * scale).round().max(1.0) as u32,
            (rect.height() * scale).round().max(1.0) as u32,
        );
        if self.editor_size_sent.get(&host_id) == Some(&wanted) {
            return;
        }
        self.editor_size_sent.insert(host_id, wanted);
        if let Some(hosted) = self.hosted.get_mut(&host_id) {
            let _ = hosted.resize_editor(wanted.0, wanted.1);
        }
    }

    /// Detaches the live editor (the plug-in keeps running headless).
    pub(crate) fn detach_editor(&mut self) {
        if let Some(id) = self.attached_editor.take() {
            if let Some(ui) = self.hosted.get_mut(&id) {
                ui.close_editor();
            }
            if let Some(window) = self.editor_windows.get_mut(&id) {
                window.set_visible(false);
            }
        }
    }

    /// Keeps the native editor window in sync with the egui wrapper, attaching
    /// it the first time the wrapper is laid out.
    pub(crate) fn sync_editor_window(&mut self, host_id: u64, rect: egui::Rect) {
        let scale = self.last_pixels_per_point.max(0.5);
        let x = (rect.min.x * scale).round() as i32;
        let y = (rect.min.y * scale).round() as i32;
        let width = (rect.width() * scale).round().max(32.0) as i32;
        let height = (rect.height() * scale).round().max(32.0) as i32;
        if self.attached_editor == Some(host_id) {
            if let Some(window) = self.editor_windows.get_mut(&host_id) {
                window.set_rect(x, y, width, height);
                window.set_visible(true);
            }
            return;
        }
        let Some(ui) = self.hosted.get_mut(&host_id) else {
            return;
        };
        let Some(window) =
            pulseforge_hosting::editor::EditorWindow::new(self.main_hwnd, x, y, width, height)
        else {
            self.hosted_errors.insert(
                host_id,
                "Could not create a native window for the plug-in editor.".to_owned(),
            );
            return;
        };
        match ui.open_editor(
            pulseforge_hosting::ParentWindow(window.handle()),
            width as u32,
            height as u32,
        ) {
            Ok((real_width, real_height)) => {
                // Plug-ins may ignore the requested size; remember the real one
                // so the wrapper reserves enough room next frame.
                if let Some(meta) = self.hosted_meta.get_mut(&host_id) {
                    meta.editor_size = (real_width, real_height);
                }
                window.set_rect(x, y, real_width as i32, real_height as i32);
                self.editor_windows.insert(host_id, window);
                self.attached_editor = Some(host_id);
            }
            Err(error) => {
                self.hosted_errors.insert(host_id, error.to_string());
            }
        }
    }
}

impl PulseForgeApp {
    /// Asks every hosted plug-in for its opaque state and stores it in the
    /// project, so a reload restores exactly what the user had.
    pub(crate) fn capture_plugin_states(&mut self) {
        for instrument_id in 0..self.project.instruments.len() {
            if self.project.instruments[instrument_id]
                .plugin_path
                .is_empty()
            {
                continue;
            }
            let id = pulseforge_core::hosted::instrument_host_id(instrument_id);
            if let Some(state) = self
                .hosted
                .get_mut(&id)
                .and_then(pulseforge_hosting::HostedUi::save_state)
            {
                self.project.instruments[instrument_id].plugin_state = state;
            }
        }
        let lane_count = self.project.lanes.len();
        for channel in 0..=lane_count {
            let slot_count = if channel == lane_count {
                self.project.master.inserts.len()
            } else {
                self.project.lanes[channel].inserts.len()
            };
            for slot in 0..slot_count {
                let has_plugin = self
                    .effect_slot_name(channel, slot)
                    .is_some_and(|_| self.effect_slot_path(channel, slot).is_some());
                if !has_plugin {
                    continue;
                }
                let id = pulseforge_core::hosted::effect_host_id(channel, slot);
                if let Some(state) = self
                    .hosted
                    .get_mut(&id)
                    .and_then(pulseforge_hosting::HostedUi::save_state)
                    && let Some(effect) = self.effect_slot_mut(channel, slot)
                {
                    effect.plugin_state = state;
                }
            }
        }
    }

    /// Plug-in path of an insert slot, when it holds a hosted bundle.
    pub(crate) fn effect_slot_path(&self, channel: usize, slot: usize) -> Option<String> {
        let slots = if channel == self.project.lanes.len() {
            &self.project.master.inserts
        } else {
            &self.project.lanes.get(channel)?.inserts
        };
        let path = &slots.get(slot)?.plugin_path;
        (!path.is_empty()).then(|| path.clone())
    }
}

