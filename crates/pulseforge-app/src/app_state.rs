//! The editor state itself.

use crate::*;

/// Which family of parameters the automation window's picker shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AutomationTargetKind {
    /// Instrument and synth parameters.
    #[default]
    Instrument,
    /// Insert-effect parameters of a channel.
    InsertEffect,
    /// Mixer channel controls: gain, pan, width, EQ bands and sends.
    Mixer,
    /// Audio clip controls and the sample defaults every clip inherits.
    Clip,
}

pub(crate) struct PulseForgeApp {
    pub(crate) project: Project,
    pub(crate) audio: Option<AudioOutput>,
    pub(crate) audio_error: Option<String>,
    pub(crate) playing: bool,
    pub(crate) windows: EditorWindows,
    pub(crate) open_instrument_plugins: BTreeSet<usize>,
    pub(crate) open_effect_plugins: BTreeSet<(usize, usize)>,
    pub(crate) plugin_selector_target: Option<PluginTarget>,
    pub(crate) selected_pattern: usize,
    pub(crate) selected_pattern_playback: bool,
    pub(crate) audio_preview_pattern: Option<usize>,
    pub(crate) arrangement_loop: Option<(u64, u64)>,
    pub(crate) loop_drag_start: Option<u64>,
    /// True once the loop drag actually moved, so a plain click keeps the
    /// previous loop instead of creating a one-step one.
    pub(crate) loop_drag_moved: bool,
    pub(crate) loop_enabled: bool,
    pub(crate) audio_loop_range: Option<(u64, u64)>,
    pub(crate) selected_instrument: usize,
    pub(crate) selected_lane: usize,
    pub(crate) selected_mixer_channel: usize,
    pub(crate) automation_target_kind: AutomationTargetKind,
    pub(crate) automation_instrument_id: usize,
    pub(crate) automation_instrument_parameter: InstrumentParameter,
    pub(crate) automation_channel_id: usize,
    pub(crate) automation_slot_id: usize,
    pub(crate) automation_effect_parameter: EffectParameter,
    /// Mixer channel the mixer picker points at, the master being `lanes.len()`.
    pub(crate) automation_mixer_channel_id: usize,
    pub(crate) automation_mixer_parameter: MixerParameter,
    /// Arrangement lane and clip the clip picker points at.
    pub(crate) automation_clip_lane: usize,
    pub(crate) automation_clip_id: usize,
    pub(crate) automation_clip_parameter: ClipParameter,
    /// True when the clip picker targets the project-wide sample defaults.
    pub(crate) automation_sample_defaults: bool,
    pub(crate) selection: Option<Selection>,
    /// Additional selected items, so shift/ctrl can build a multi-selection.
    pub(crate) extra_selection: Vec<Selection>,
    /// Anchor used for ctrl+click range selection.
    pub(crate) selection_anchor: Option<Selection>,
    /// Instrument being renamed inline in the channel rack.
    pub(crate) renaming_instrument: Option<usize>,
    /// Cached peak envelope for the sample shown in the sample-player window.
    pub(crate) sample_peaks: Option<(String, Vec<f32>)>,
    /// Cached bundle metadata for the hosted plug-in window.
    pub(crate) plugin_metadata: Option<(String, pulseforge_plugins::PluginMetadata)>,
    /// UI halves of loaded VST3/CLAP plug-ins, keyed by host id.
    pub(crate) hosted: std::collections::HashMap<u64, pulseforge_hosting::HostedUi>,
    /// Static data read once per load; querying it per frame is expensive for
    /// bundles with hundreds of parameters.
    pub(crate) hosted_meta: std::collections::HashMap<u64, HostedMeta>,
    /// Native child windows holding each hosted plug-in's editor.
    pub(crate) editor_windows:
        std::collections::HashMap<u64, pulseforge_hosting::editor::EditorWindow>,
    /// Last hosting error per slot, shown instead of a dead editor surface.
    pub(crate) hosted_errors: std::collections::HashMap<u64, String>,
    /// Editor currently attached, so it can be detached before its window dies.
    pub(crate) attached_editor: Option<u64>,
    /// Plug-in parameter last touched in a plug-in's own editor, with its name.
    pub(crate) last_tweaked: Option<(u64, String)>,
    /// Arrangement clip whose sample window is open.
    pub(crate) sample_clip_window: Option<(usize, usize)>,
    /// Size last requested from a plug-in editor, to avoid redundant resizes.
    pub(crate) editor_size_sent: std::collections::HashMap<u64, (u32, u32)>,
    /// HWND of the DAW window, captured once per frame.
    pub(crate) main_hwnd: isize,
    /// Pixels per point from the last frame, needed to place native windows.
    pub(crate) last_pixels_per_point: f32,
    /// Rubber-band origin for the select tool.
    pub(crate) box_select_start: Option<egui::Pos2>,
    /// Rubber-band origin (tick, pitch) for ctrl+drag in the piano roll.
    pub(crate) piano_box_origin: Option<(u64, u8)>,
    /// Modifier state sampled once per frame for selection helpers.
    pub(crate) mod_shift: bool,
    pub(crate) mod_ctrl: bool,
    pub(crate) arrangement_pan_steps: u64,
    pub(crate) arrangement_pan_lanes: usize,
    pub(crate) pattern_pan_steps: u64,
    pub(crate) piano_pan_steps: u64,
    pub(crate) piano_tool: PianoTool,
    pub(crate) piano_gesture: Option<PianoGesture>,
    pub(crate) rack_gesture: Option<RackGesture>,
    pub(crate) note_length_steps: u64,
    pub(crate) pattern_drag: Option<ClipSource>,
    /// Source placed by the arrangement pen and brush tools, mirroring the
    /// "last clicked item" behaviour of FL Studio.
    pub(crate) arrangement_source: Option<ClipSource>,
    pub(crate) arrangement_gesture: Option<ArrangementGesture>,
    /// True while the arrangement brush is painting a stroke.
    pub(crate) arrangement_painting: bool,
    /// Undo/redo snapshots of the whole project model.
    pub(crate) history: Vec<Project>,
    pub(crate) redo_history: Vec<Project>,
    /// Smoothed transport position for the animated playhead.
    pub(crate) playhead_display: f64,
    pub(crate) playhead_raw: u64,
    pub(crate) playhead_anchor: std::time::Instant,
    pub(crate) metronome: bool,
    /// Pending project snapshot plus the throttle timestamp for coalescing.
    pub(crate) project_dirty: bool,
    pub(crate) last_project_sync: std::time::Instant,
    pub(crate) settings: Settings,
    pub(crate) settings_path: String,
    pub(crate) settings_tab: SettingsTab,
    /// Cached external plug-ins discovered from the configured locations.
    pub(crate) external_plugins: Vec<pulseforge_plugins::ExternalPlugin>,
    pub(crate) plugin_scan_error: Option<String>,
    /// Chord being captured for a shortcut row, if any.
    pub(crate) capturing_shortcut: Option<ShortcutAction>,
    pub(crate) shortcut_draft: String,
    pub(crate) sample_tab: SampleTab,
    /// Files found under the configured sample folders.
    pub(crate) sample_files: Vec<String>,
    /// Animated visual state: clip lift, knob glow and panel fades.
    /// Track id being renamed, plus the in-progress text.
    pub(crate) renaming_track: Option<usize>,
    pub(crate) rename_buffer: String,
    /// Automation lane currently being dragged in the arrangement.
    pub(crate) automation_gesture: Option<AutomationGesture>,
    pub(crate) clip_lift: Animated,
    pub(crate) drag_ghost: Animated,
    pub(crate) inspector_fade: Animated,
    pub(crate) last_frame_seconds: f32,
    /// Set while a track height drag is in flight so it syncs once on release.
    pub(crate) track_resize_dirty: bool,
    /// Note repeat settings applied to newly drawn notes.
    /// Tool used by the clip arrangement (Join stays piano-roll only).
    pub(crate) arrangement_tool: PianoTool,
    /// Which control lane is shown under the piano roll.
    pub(crate) control_lane: ControlLane,
    /// Hides the control lane, giving the grid the full height.
    pub(crate) control_lane_collapsed: bool,
    /// Search filters for the settings and plug-in picker lists.
    pub(crate) settings_search: String,
    pub(crate) shortcut_search: String,
    pub(crate) plugin_search: String,
    /// Which tab a plug-in window shows.
    pub(crate) plugin_settings_tab: std::collections::BTreeSet<PluginTarget>,
    /// PluginTab::Settings ids are tracked here.
    /// Ruler drag used to select notes; it never touches the transport loop.
    pub(crate) piano_select_start: Option<u64>,
    pub(crate) piano_selection: Option<(u64, u64)>,
    pub(crate) note_repeat_count: u8,
    pub(crate) note_repeat_ticks: u8,
    pub(crate) note_repeat_decay: u8,
    pub(crate) routing_drag_from: Option<usize>,
    /// Route whose send knob is being dragged; latched so the knob survives
    /// the pointer leaving the cable.
    pub(crate) routing_gain_route: Option<(usize, usize)>,
    pub(crate) new_audio_path: String,
    pub(crate) new_audio_length_steps: u64,
    /// Text of the automation window's formula field.
    pub(crate) formula_text: String,
    /// Name typed into the mixer inspector's preset field.
    pub(crate) chain_preset_name: String,
    /// Presets found on disk, refreshed when the inspector saves or loads.
    pub(crate) chain_presets: Vec<String>,
    pub(crate) render_path: String,
    pub(crate) project_path: String,
    pub(crate) status: String,
    pub(crate) midi_ports: Vec<String>,
    pub(crate) midi_discovery_error: Option<String>,
    pub(crate) selected_midi_port: Option<usize>,
    pub(crate) midi_connection: Option<MidiInputConnection<()>>,
    pub(crate) midi_routes: Arc<Mutex<Vec<usize>>>,
    pub(crate) midi_callback_error: Arc<Mutex<Option<String>>>,
    /// Background jobs: plug-in scans, WAV decoding and project IO.
    pub(crate) tasks: tasks::Tasks,
    /// Per-slot timing counters for hosted VST3/CLAP plug-ins.
    pub(crate) plugin_debug: PluginDebug,
    /// The debug/profiler window plus its graph history.
    pub(crate) debug_panel: DebugPanel,
    /// Compact live numbers shown in the footer while the panel is open.
    pub(crate) debug_summary: String,
    /// CPU/RAM sampler running on its own thread.
    pub(crate) system_monitor: SystemMonitor,
}
