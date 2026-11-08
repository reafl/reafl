//! Constructing the editor and applying its start-up settings.

use crate::*;

impl ReaflApp {
    pub(crate) fn new(creation_context: &eframe::CreationContext<'_>, cli: &Cli) -> Self {
        // The settings path comes from the command line, so a portable install
        // (or a CI run) can point at its own configuration.
        let settings_path = cli.settings_path();
        let settings = Settings::load(&settings_path);
        apply_theme(&creation_context.egui_ctx, &settings.theme);
        let project = Project::default();
        // `--no-audio` keeps the engine from opening a device: hosted plug-ins
        // are then never instantiated, which is the safe way to open a project
        // that crashes a plug-in.
        let (audio, audio_error) = if cli.no_audio {
            (
                None,
                Some("Audio disabled (--no-audio); plug-ins stay unloaded.".to_owned()),
            )
        } else {
            match AudioOutput::start(&project) {
                Ok(output) => (Some(output), None),
                Err(error) => (None, Some(error)),
            }
        };
        let (midi_ports, midi_discovery_error) = match Self::query_midi_ports() {
            Ok(ports) => (ports, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        if let Some(audio) = &audio {
            let _ = audio.sender.try_send(AudioCommand::SetSmoothing {
                enabled: settings.parameter_smoothing,
                milliseconds: settings.smoothing_ms,
            });
        }
        let mut app = Self {
            midi_routes: Arc::new(Mutex::new(project.midi_routes.to_vec())),
            project,
            audio,
            audio_error,
            playing: false,
            windows: EditorWindows {
                channel_rack: true,
                ..Default::default()
            },
            open_instrument_plugins: BTreeSet::new(),
            open_effect_plugins: BTreeSet::new(),
            plugin_selector_target: None,
            selected_clip: 0,
            selected_clip_playback: false,
            audio_preview_pattern: None,
            arrangement_loop: None,
            loop_drag_start: None,
            loop_drag_moved: false,
            loop_enabled: false,
            audio_loop_range: None,
            selected_instrument: 0,
            selected_lane: 0,
            selected_mixer_channel: 0,
            automation_target_kind: AutomationTargetKind::Instrument,
            automation_track_id: 0,
            automation_track_parameter: TrackParameter::Generator(InstrumentParameter::Gain),
            automation_channel_id: 0,
            automation_slot_id: 0,
            automation_effect_parameter: EffectParameter::Mix,
            automation_mixer_channel_id: 0,
            automation_mixer_parameter: MixerParameter::Gain,
            automation_clip_lane: 0,
            automation_clip_id: 0,
            automation_clip_parameter: ClipParameter::Gain,
            automation_sample_defaults: true,
            selection: None,
            extra_selection: Vec::new(),
            selection_anchor: None,
            renaming_instrument: None,
            sample_peaks: None,
            plugin_metadata: None,
            hosted: std::collections::HashMap::new(),
            hosted_meta: std::collections::HashMap::new(),
            editor_windows: std::collections::HashMap::new(),
            hosted_errors: std::collections::HashMap::new(),
            attached_editor: None,
            last_tweaked: None,
            sample_clip_window: None,
            editor_size_sent: std::collections::HashMap::new(),
            main_hwnd: 0,
            last_pixels_per_point: 1.0,
            box_select_start: None,
            piano_box_origin: None,
            mod_shift: false,
            mod_ctrl: false,
            arrangement_pan_steps: 0,
            arrangement_pan_lanes: 0,
            pattern_pan_steps: 0,
            piano_pan_steps: 0,
            piano_tool: PianoTool::Pen,
            piano_gesture: None,
            rack_gesture: None,
            note_length_steps: 4,
            pattern_drag: None,
            arrangement_source: Some(ClipPlacement::Notes { length_steps: 16 }),
            arrangement_gesture: None,
            arrangement_painting: false,
            history: Vec::new(),
            redo_history: Vec::new(),
            playhead_display: 0.0,
            playhead_raw: 0,
            playhead_anchor: std::time::Instant::now(),
            metronome: false,
            project_dirty: false,
            last_project_sync: std::time::Instant::now(),
            settings,
            settings_path,
            settings_tab: SettingsTab::default(),
            external_plugins: Vec::new(),
            plugin_scan_error: None,
            capturing_shortcut: None,
            shortcut_draft: String::new(),
            sample_tab: SampleTab::default(),
            sample_files: Vec::new(),
            renaming_track: None,
            rename_buffer: String::new(),
            automation_gesture: None,
            clip_lift: Animated::new(0.0),
            drag_ghost: Animated::new(0.0),
            inspector_fade: Animated::new(0.0),
            last_frame_seconds: 1.0 / 60.0,
            track_resize_dirty: false,
            arrangement_tool: PianoTool::Pen,
            control_lane: ControlLane::Velocity,
            control_lane_collapsed: false,
            settings_search: String::new(),
            shortcut_search: String::new(),
            plugin_search: String::new(),
            plugin_settings_tab: std::collections::BTreeSet::new(),
            piano_select_start: None,
            piano_selection: None,
            note_repeat_count: 0,
            note_repeat_ticks: 0,
            note_repeat_decay: 8,
            routing_drag_from: None,
            routing_gain_route: None,
            new_audio_path: String::new(),
            new_audio_length_steps: 64,
            render_path: "reafl.wav".to_owned(),
            formula_text: "0.5 + 0.4 * sin(2 * pi * x)".to_owned(),
            chain_preset_name: "My chain".to_owned(),
            chain_presets: list_chain_presets(),
            project_path: "session.json".to_owned(),
            status: "Ready · drag a pattern or audio clip onto the timeline".to_owned(),
            midi_ports,
            midi_discovery_error,
            selected_midi_port: None,
            midi_connection: None,
            midi_callback_error: Arc::new(Mutex::new(None)),
            tasks: tasks::Tasks::new(),
            plugin_debug: PluginDebug::default(),
            debug_panel: DebugPanel::new(cli.debug),
            debug_summary: String::new(),
            system_monitor: SystemMonitor::start(),
        };
        // Start-up work runs off the UI thread: a large plug-in folder or
        // sample library no longer delays the first frame.
        app.rescan_plugins();
        app.refresh_sample_files();
        if let Some(path) = cli.project_path() {
            app.project_path = path;
            if std::path::Path::new(&app.project_path).exists() {
                app.load_project();
            } else {
                app.status = format!("Project {} not found; starting empty.", app.project_path);
            }
        }
        app
    }

    /// Queues a plug-in folder scan on a worker thread. The status line is set
    /// immediately and the result arrives through [`Self::apply_task`].
    pub(crate) fn rescan_plugins(&mut self) {
        let roots = self.settings.plugin_roots();
        let settings_missing = roots.is_empty();
        self.status = "Scanning plug-in folders…".to_owned();
        self.tasks.scan_plugins(roots, settings_missing);
    }

    pub(crate) fn save_settings(&mut self) {
        match self.settings.save(&self.settings_path) {
            Ok(()) => self.status = format!("Settings saved to {}.", self.settings_path),
            Err(error) => self.status = error,
        }
    }

    /// Pushes the settings that affect the audio engine.
    pub(crate) fn apply_audio_settings(&mut self) {
        self.send_audio(AudioCommand::SetSmoothing {
            enabled: self.settings.parameter_smoothing,
            milliseconds: self.settings.smoothing_ms,
        });
    }

    /// Refreshes the sample browser file list from the configured folders.
    /// Directory walking happens on a worker thread so a network share or a
    /// huge library cannot stall a frame.
    pub(crate) fn refresh_sample_files(&mut self) {
        let roots = self.settings.sample_paths.clone();
        self.status = "Scanning sample folders…".to_owned();
        self.tasks.scan_samples(roots);
    }

    /// Auditions a sample through the master bus.
    pub(crate) fn preview_sample(&mut self, path: &str) {
        self.status = format!("Loading {path}…");
        self.tasks
            .decode_sample(path.to_owned(), tasks::SampleUse::Preview);
    }
}
