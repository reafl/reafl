//! Persistent application settings.
//!
//! Everything the settings window edits lives here and is stored in
//! `settings.json`. Keyboard bindings are data, not code: [`ShortcutAction`]
//! lists every command, and [`Settings::bind`] assigns a chord to one, so a new
//! command only needs an enum variant plus a handler.

use std::fs;

use serde::{Deserialize, Serialize};

/// File name used inside the working directory.
pub const SETTINGS_FILE: &str = "settings.json";

/// Every command that can be bound to a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutAction {
    // Transport
    TransportToggle,
    TransportStop,
    ToggleLoop,
    TogglePatternPlayback,
    ToggleMetronome,
    SnapUp,
    SnapDown,
    // Windows
    WindowChannelRack,
    WindowPianoRoll,
    WindowMixer,
    WindowRouting,
    WindowAutomation,
    WindowMidi,
    WindowSamples,
    WindowSettings,
    WindowRender,
    /// Debug/profiler panel (CPU, RAM, flame graph, plug-in timings).
    WindowDebug,
    // Tools
    ToolPen,
    ToolSelect,
    ToolBrush,
    ToolJoin,
    ToolCut,
    // Piano roll control lane
    ControlVelocity,
    ControlRepeats,
    // Note editing
    NoteUp,
    NoteDown,
    NoteOctaveUp,
    NoteOctaveDown,
    NoteLeft,
    NoteRight,
    // Editing
    Undo,
    Redo,
    SelectAll,
    DuplicateClip,
    DeleteSelection,
    RenameTrack,
    RouteToFreeInsert,
    ClearSelection,
    ResetPan,
    SaveProject,
    LoadProject,
}

impl ShortcutAction {
    /// Stable label used in the settings window and `settings.json`.
    pub fn label(self) -> &'static str {
        match self {
            Self::TransportToggle => "Play / stop",
            Self::TransportStop => "Stop and rewind",
            Self::ToggleLoop => "Toggle arrangement loop",
            Self::TogglePatternPlayback => "Toggle pattern playback",
            Self::ToggleMetronome => "Toggle metronome",
            Self::SnapUp => "Increase snap size",
            Self::SnapDown => "Decrease snap size",
            Self::WindowChannelRack => "Channel rack window",
            Self::WindowPianoRoll => "Piano roll window",
            Self::WindowMixer => "Mixer window",
            Self::WindowRouting => "Mixer routing window",
            Self::WindowAutomation => "Automation window",
            Self::WindowMidi => "MIDI routing window",
            Self::WindowSamples => "Sample browser window",
            Self::WindowSettings => "Settings window",
            Self::WindowRender => "Render window",
            Self::WindowDebug => "Debug / profiler window",
            Self::ToolPen => "Pen tool",
            Self::ToolSelect => "Select tool",
            Self::ToolBrush => "Brush tool",
            Self::ToolJoin => "Join tool",
            Self::ToolCut => "Cut tool",
            Self::ControlVelocity => "Show velocity lane",
            Self::ControlRepeats => "Show repeats lane",
            Self::NoteUp => "Move notes up a semitone",
            Self::NoteDown => "Move notes down a semitone",
            Self::NoteOctaveUp => "Move notes up an octave",
            Self::NoteOctaveDown => "Move notes down an octave",
            Self::NoteLeft => "Move notes left by the snap",
            Self::NoteRight => "Move notes right by the snap",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::SelectAll => "Select all notes",
            Self::DuplicateClip => "Duplicate selection",
            Self::DeleteSelection => "Delete selection",
            Self::RenameTrack => "Rename track",
            Self::RouteToFreeInsert => "Route to free mixer insert",
            Self::ClearSelection => "Clear selection",
            Self::ResetPan => "Reset view pan",
            Self::SaveProject => "Save project",
            Self::LoadProject => "Open project",
        }
    }

    /// Group heading used to organise the settings list.
    pub fn group(self) -> &'static str {
        match self {
            Self::TransportToggle
            | Self::TransportStop
            | Self::ToggleLoop
            | Self::TogglePatternPlayback
            | Self::ToggleMetronome
            | Self::SnapUp
            | Self::SnapDown => "Transport",
            Self::WindowChannelRack
            | Self::WindowPianoRoll
            | Self::WindowMixer
            | Self::WindowRouting
            | Self::WindowAutomation
            | Self::WindowMidi
            | Self::WindowSamples
            | Self::WindowSettings
            | Self::WindowRender
            | Self::WindowDebug => "Windows",
            Self::ToolPen | Self::ToolSelect | Self::ToolBrush | Self::ToolJoin | Self::ToolCut => {
                "Tools"
            }
            Self::ControlVelocity
            | Self::ControlRepeats
            | Self::NoteUp
            | Self::NoteDown
            | Self::NoteOctaveUp
            | Self::NoteOctaveDown
            | Self::NoteLeft
            | Self::NoteRight => "Piano roll",
            Self::Undo
            | Self::Redo
            | Self::SelectAll
            | Self::DuplicateClip
            | Self::DeleteSelection
            | Self::RenameTrack
            | Self::RouteToFreeInsert
            | Self::ClearSelection
            | Self::ResetPan
            | Self::SaveProject
            | Self::LoadProject => "Editing",
        }
    }

    /// Every action, in the order the settings window lists them.
    pub fn all() -> &'static [ShortcutAction] {
        &[
            Self::TransportToggle,
            Self::TransportStop,
            Self::ToggleLoop,
            Self::TogglePatternPlayback,
            Self::ToggleMetronome,
            Self::SnapUp,
            Self::SnapDown,
            Self::WindowChannelRack,
            Self::WindowPianoRoll,
            Self::WindowMixer,
            Self::WindowRouting,
            Self::WindowAutomation,
            Self::WindowMidi,
            Self::WindowSamples,
            Self::WindowSettings,
            Self::WindowRender,
            Self::WindowDebug,
            Self::ToolPen,
            Self::ToolSelect,
            Self::ToolBrush,
            Self::ToolJoin,
            Self::ToolCut,
            Self::ControlVelocity,
            Self::ControlRepeats,
            Self::NoteUp,
            Self::NoteDown,
            Self::NoteOctaveUp,
            Self::NoteOctaveDown,
            Self::NoteLeft,
            Self::NoteRight,
            Self::Undo,
            Self::Redo,
            Self::SelectAll,
            Self::DuplicateClip,
            Self::DeleteSelection,
            Self::RenameTrack,
            Self::RouteToFreeInsert,
            Self::ClearSelection,
            Self::ResetPan,
            Self::SaveProject,
            Self::LoadProject,
        ]
    }
}

/// A parsed key chord such as `Ctrl+Shift+K`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Option<&'static str>,
}

/// Key names a binding may use, mapped to egui keys by the UI layer.
pub const KEY_NAMES: &[&str] = &[
    "Space", "Enter", "Tab", "Escape", "Delete", "Home", "F2", "F5", "F6", "F7", "F8", "F9", "F10",
    "F11", "F12", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P",
    "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "0", "1", "2", "3", "4", "5", "6", "7", "8",
    "9", "Comma", "Period", "Minus",
];

impl Chord {
    /// Parses `Ctrl+Alt+Shift+Key` in any order. Unknown parts are ignored.
    pub fn parse(text: &str) -> Self {
        let mut chord = Self::default();
        for part in text.split('+') {
            let part = part.trim();
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" | "option" => chord.alt = true,
                "" => {}
                _ => {
                    let canonical = KEY_NAMES
                        .iter()
                        .find(|name| name.eq_ignore_ascii_case(part));
                    chord.key = canonical.copied();
                }
            }
        }
        chord
    }

    /// Renders the chord back to its canonical text form.
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        if let Some(key) = self.key {
            parts.push(key.to_owned());
        }
        parts.join("+")
    }
}

/// One command-to-chord assignment.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutBinding {
    pub action: ShortcutAction,
    pub keys: String,
    pub enabled: bool,
}

impl Default for ShortcutBinding {
    fn default() -> Self {
        Self {
            action: ShortcutAction::TransportToggle,
            keys: String::new(),
            enabled: true,
        }
    }
}

impl ShortcutBinding {
    pub fn chord(&self) -> Chord {
        Chord::parse(&self.keys)
    }

    /// Convenience for creating bindings in code.
    pub fn new(action: ShortcutAction, keys: &str) -> Self {
        Self {
            action,
            keys: keys.to_owned(),
            enabled: true,
        }
    }
}

/// Everything the settings window edits.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub shortcuts: Vec<ShortcutBinding>,
    /// Extra directories scanned for VST 2 plug-ins.
    pub vst_paths: Vec<String>,
    /// Extra directories scanned for VST3 plug-ins.
    pub vst3_paths: Vec<String>,
    /// Extra directories scanned for CLAP plug-ins.
    pub clap_paths: Vec<String>,
    /// Directories the sample browser lists.
    pub sample_paths: Vec<String>,
    /// Linear parameter ramping; disables the click-free interpolation.
    pub parameter_smoothing: bool,
    /// Ramp length in milliseconds.
    pub smoothing_ms: f32,
    /// UI transitions such as dragging clips.
    pub animations: bool,
    /// Speed multiplier for UI transitions.
    pub animation_speed: f32,
    /// Metronome click level, 0-1.
    pub metronome_volume: f32,
    /// Vertical zoom of the arrangement.
    pub default_track_height: f32,
    /// MIDI velocity applied to newly drawn notes.
    pub default_velocity: u8,
    /// How tooltips are presented.
    pub tooltips: TooltipMode,
    /// Seconds the pointer must rest before a tooltip appears.
    pub tooltip_delay: f32,
    /// Bouncy transitions on drags and selections.
    pub fancy_animations: bool,
    /// Arrangement horizontal zoom multiplier.
    pub zoom: f32,
    /// DPI-like interface scale applied to the whole UI.
    pub ui_scale: f32,
    /// Step size used by Alt+wheel, in ticks.
    pub wheel_step_ticks: u64,
    /// Theme accent colour, edited on the (placeholder) Theming page.
    pub accent: [u8; 3],
    /// Placeholder switches for the general options page.
    pub restore_last_project: bool,
    pub autosave_minutes: u32,
    /// Root note of the piano-roll scale highlight, 0 = C.
    pub scale_root: u8,
    /// Scale used for highlighting and note snapping.
    pub scale_name: String,
    /// Snap drawn and moved notes to the selected scale.
    pub scale_snap: bool,
    /// Active theme.
    pub theme: Theme,
}

/// Colours and typography the UI draws with. Everything here is data, so a
/// theme only changes appearance and never behaviour.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    /// Accent used for highlights, selection and active controls.
    pub accent: [u8; 3],
    /// Window background behind panels.
    pub background: [u8; 3],
    /// Panel and control background.
    pub panel: [u8; 3],
    /// MIDI note and clip fill colour.
    pub note: [u8; 3],
    /// Meter gradient, quiet to loud.
    pub meter_low: [u8; 3],
    pub meter_mid: [u8; 3],
    pub meter_high: [u8; 3],
    /// Primary text colour.
    pub text: [u8; 3],
    /// Multiplier on the default font sizes.
    pub font_scale: f32,
    /// Optional background image path drawn behind the arrangement.
    pub background_image: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "PulseForge Dark".to_owned(),
            accent: [112, 208, 190],
            background: [22, 26, 33],
            panel: [29, 33, 41],
            note: [122, 102, 193],
            meter_low: [83, 193, 145],
            meter_mid: [225, 177, 80],
            meter_high: [225, 83, 75],
            text: [220, 224, 230],
            font_scale: 1.0,
            background_image: String::new(),
        }
    }
}

impl Theme {
    /// Builds an egui colour from one of the theme's channel triples.
    pub fn color(&self, channels: [u8; 3]) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(channels[0], channels[1], channels[2])
    }

    /// A handful of ready-made presets offered in the theming page.
    pub fn presets() -> [Self; 4] {
        [
            Self::default(),
            Self {
                name: "Studio Light".to_owned(),
                accent: [42, 122, 168],
                background: [238, 240, 244],
                panel: [250, 251, 253],
                note: [86, 128, 200],
                meter_low: [60, 170, 110],
                meter_mid: [220, 165, 60],
                meter_high: [210, 70, 60],
                text: [30, 34, 40],
                font_scale: 1.0,
                background_image: String::new(),
            },
            Self {
                name: "Amber".to_owned(),
                accent: [233, 168, 76],
                background: [26, 22, 18],
                panel: [36, 31, 25],
                note: [214, 138, 66],
                meter_low: [120, 190, 130],
                meter_mid: [235, 180, 80],
                meter_high: [225, 90, 60],
                text: [240, 226, 208],
                font_scale: 1.0,
                background_image: String::new(),
            },
            Self {
                name: "High contrast".to_owned(),
                accent: [255, 214, 0],
                background: [0, 0, 0],
                panel: [16, 16, 16],
                note: [255, 214, 0],
                meter_low: [0, 255, 120],
                meter_mid: [255, 214, 0],
                meter_high: [255, 40, 40],
                text: [255, 255, 255],
                font_scale: 1.1,
                background_image: String::new(),
            },
        ]
    }
}

/// Where tooltips appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TooltipMode {
    /// Never show hover tooltips; details appear in the status bar instead.
    #[default]
    StatusBar,
    /// Show after `tooltip_delay` seconds of hovering.
    Hover,
    /// Show immediately on hover.
    Immediate,
}

impl TooltipMode {
    pub const ALL: [Self; 3] = [Self::StatusBar, Self::Hover, Self::Immediate];

    pub fn label(self) -> &'static str {
        match self {
            Self::StatusBar => "Status bar only",
            Self::Hover => "Hover after a delay",
            Self::Immediate => "Hover immediately",
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcuts: default_shortcuts(),
            vst_paths: Vec::new(),
            vst3_paths: Vec::new(),
            clap_paths: Vec::new(),
            sample_paths: Vec::new(),
            parameter_smoothing: true,
            smoothing_ms: 12.0,
            animations: true,
            animation_speed: 1.0,
            metronome_volume: 0.5,
            default_track_height: 54.0,
            default_velocity: 100,
            tooltips: TooltipMode::default(),
            tooltip_delay: 0.6,
            fancy_animations: false,
            zoom: 1.0,
            ui_scale: 1.0,
            wheel_step_ticks: 4,
            accent: [112, 208, 190],
            restore_last_project: false,
            autosave_minutes: 0,
            scale_root: 0,
            scale_name: "Chromatic".to_owned(),
            scale_snap: false,
            theme: Theme::default(),
        }
    }
}

/// The shipped key map. This is the only place default chords are declared.
pub fn default_shortcuts() -> Vec<ShortcutBinding> {
    vec![
        ShortcutBinding::new(ShortcutAction::TransportToggle, "Space"),
        ShortcutBinding::new(ShortcutAction::ToggleLoop, "L"),
        ShortcutBinding::new(ShortcutAction::TogglePatternPlayback, "Ctrl+Shift+P"),
        ShortcutBinding::new(ShortcutAction::ToggleMetronome, "Ctrl+M"),
        ShortcutBinding::new(ShortcutAction::SnapUp, "Shift+Period"),
        ShortcutBinding::new(ShortcutAction::SnapDown, "Shift+Comma"),
        ShortcutBinding::new(ShortcutAction::WindowChannelRack, "F6"),
        ShortcutBinding::new(ShortcutAction::WindowPianoRoll, "F7"),
        ShortcutBinding::new(ShortcutAction::WindowMixer, "F9"),
        ShortcutBinding::new(ShortcutAction::WindowRouting, "Shift+F9"),
        ShortcutBinding::new(ShortcutAction::WindowAutomation, "F10"),
        ShortcutBinding::new(ShortcutAction::WindowMidi, "F8"),
        ShortcutBinding::new(ShortcutAction::WindowSamples, "F11"),
        ShortcutBinding::new(ShortcutAction::WindowDebug, "Ctrl+Shift+D"),
        ShortcutBinding::new(ShortcutAction::WindowSettings, "Ctrl+Comma"),
        ShortcutBinding::new(ShortcutAction::ToolPen, "P"),
        ShortcutBinding::new(ShortcutAction::ToolSelect, "S"),
        ShortcutBinding::new(ShortcutAction::ToolBrush, "B"),
        ShortcutBinding::new(ShortcutAction::ToolJoin, "J"),
        ShortcutBinding::new(ShortcutAction::ToolCut, "C"),
        ShortcutBinding::new(ShortcutAction::ControlVelocity, "Ctrl+Shift+V"),
        ShortcutBinding::new(ShortcutAction::ControlRepeats, "Ctrl+Shift+R"),
        ShortcutBinding::new(ShortcutAction::Undo, "Ctrl+Z"),
        ShortcutBinding::new(ShortcutAction::Redo, "Ctrl+Shift+Z"),
        ShortcutBinding::new(ShortcutAction::SelectAll, "Ctrl+A"),
        ShortcutBinding::new(ShortcutAction::DuplicateClip, "Ctrl+D"),
        ShortcutBinding::new(ShortcutAction::DeleteSelection, "Delete"),
        ShortcutBinding::new(ShortcutAction::RenameTrack, "F2"),
        ShortcutBinding::new(ShortcutAction::RouteToFreeInsert, "Ctrl+L"),
        ShortcutBinding::new(ShortcutAction::ClearSelection, "Escape"),
        ShortcutBinding::new(ShortcutAction::NoteUp, "Shift+Up"),
        ShortcutBinding::new(ShortcutAction::NoteDown, "Shift+Down"),
        ShortcutBinding::new(ShortcutAction::NoteOctaveUp, "Ctrl+Shift+Up"),
        ShortcutBinding::new(ShortcutAction::NoteOctaveDown, "Ctrl+Shift+Down"),
        ShortcutBinding::new(ShortcutAction::NoteLeft, "Shift+Left"),
        ShortcutBinding::new(ShortcutAction::NoteRight, "Shift+Right"),
        ShortcutBinding::new(ShortcutAction::ResetPan, "Home"),
        ShortcutBinding::new(ShortcutAction::SaveProject, "Ctrl+S"),
        ShortcutBinding::new(ShortcutAction::LoadProject, "Ctrl+O"),
    ]
}

impl Settings {
    /// Loads `settings.json`, falling back to defaults when it is missing or
    /// unreadable so a corrupt file never blocks startup.
    pub fn load(path: &str) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            let defaults = Self::default();
            let _ = defaults.save(path);
            return defaults;
        };
        match serde_json::from_str::<Settings>(&text) {
            Ok(mut settings) => {
                settings.normalize();
                settings
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|error| format!("Could not encode settings: {error}"))?;
        fs::write(path, json).map_err(|error| format!("Could not write {path}: {error}"))
    }

    fn normalize(&mut self) {
        self.smoothing_ms = self.smoothing_ms.clamp(0.0, 500.0);
        self.animation_speed = self.animation_speed.clamp(0.1, 4.0);
        self.metronome_volume = self.metronome_volume.clamp(0.0, 1.0);
        self.default_track_height = self.default_track_height.clamp(34.0, 260.0);
        self.default_velocity = self.default_velocity.clamp(1, 127);
        self.tooltip_delay = self.tooltip_delay.clamp(0.0, 5.0);
        self.zoom = self.zoom.clamp(0.2, 8.0);
        self.ui_scale = self.ui_scale.clamp(0.6, 2.5);
        self.wheel_step_ticks = self.wheel_step_ticks.clamp(1, 64);
        self.autosave_minutes = self.autosave_minutes.min(120);
        // An empty binding list would silently disable every shortcut.
        if self.shortcuts.is_empty() {
            self.shortcuts = default_shortcuts();
        }
        // Commands added by a newer build get their shipped chord when the
        // stored file predates them. Existing entries are never touched, so a
        // deliberate unbind (an entry with empty `keys`) survives.
        let missing: Vec<_> = default_shortcuts()
            .into_iter()
            .filter(|shipped| {
                !self
                    .shortcuts
                    .iter()
                    .any(|existing| existing.action == shipped.action)
            })
            .collect();
        self.shortcuts.extend(missing);
    }

    /// Assigns `keys` to `action`, replacing any previous chord for it.
    ///
    /// This is the programmatic entry point used by the settings window and by
    /// code that wants to create bindings without the UI.
    pub fn bind(&mut self, action: ShortcutAction, keys: &str) {
        let keys = keys.trim();
        if let Some(existing) = self
            .shortcuts
            .iter_mut()
            .find(|binding| binding.action == action)
        {
            existing.keys = keys.to_owned();
            existing.enabled = !keys.is_empty();
            return;
        }
        self.shortcuts.push(ShortcutBinding {
            action,
            keys: keys.to_owned(),
            enabled: !keys.is_empty(),
        });
    }

    /// Removes the binding for `action`.
    pub fn unbind(&mut self, action: ShortcutAction) {
        self.bind(action, "");
    }

    /// Chords currently assigned to `action`.
    pub fn keys_for(&self, action: ShortcutAction) -> &str {
        self.shortcuts
            .iter()
            .find(|binding| binding.action == action)
            .map_or("", |binding| binding.keys.as_str())
    }

    /// Every binding for a chord, used to detect conflicts in the UI.
    pub fn actions_for_chord(&self, keys: &str) -> Vec<ShortcutAction> {
        let chord = Chord::parse(keys);
        self.shortcuts
            .iter()
            .filter(|binding| binding.enabled && binding.chord() == chord)
            .map(|binding| binding.action)
            .collect()
    }

    /// All directories the plug-in scanner should search.
    pub fn plugin_roots(&self) -> Vec<String> {
        self.vst_paths
            .iter()
            .chain(&self.vst3_paths)
            .chain(&self.clap_paths)
            .filter(|path| !path.trim().is_empty())
            .cloned()
            .collect()
    }

    /// Restores the shipped key map.
    pub fn reset_shortcuts(&mut self) {
        self.shortcuts = default_shortcuts();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse_in_any_order_and_round_trip() {
        let chord = Chord::parse("Shift+Ctrl+Z");
        assert!(chord.ctrl && chord.shift && !chord.alt);
        assert_eq!(chord.key, Some("Z"));
        assert_eq!(chord.display(), "Ctrl+Shift+Z");
        assert_eq!(Chord::parse("F9").display(), "F9");
        assert_eq!(Chord::parse("ctrl+space").display(), "Ctrl+Space");
    }

    #[test]
    fn binding_an_action_replaces_the_previous_chord() {
        let mut settings = Settings::default();
        settings.bind(ShortcutAction::Undo, "Ctrl+U");
        assert_eq!(settings.keys_for(ShortcutAction::Undo), "Ctrl+U");
        let count = settings
            .shortcuts
            .iter()
            .filter(|binding| binding.action == ShortcutAction::Undo)
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn binding_an_unlisted_action_creates_it() {
        let mut settings = Settings::default();
        settings.unbind(ShortcutAction::WindowSamples);
        assert!(settings.keys_for(ShortcutAction::WindowSamples).is_empty());
        settings.bind(ShortcutAction::WindowSamples, "F12");
        assert_eq!(settings.keys_for(ShortcutAction::WindowSamples), "F12");
    }

    #[test]
    fn conflicts_are_detectable() {
        let mut settings = Settings::default();
        settings.bind(ShortcutAction::Undo, "Ctrl+K");
        assert_eq!(
            settings.actions_for_chord("Ctrl+K"),
            vec![ShortcutAction::Undo]
        );
        assert!(settings.actions_for_chord("Ctrl+Shift+Q").is_empty());
    }

    #[test]
    fn older_settings_gain_new_commands_but_keep_unbinds() {
        let mut settings = Settings::default();
        // Simulate a file written before the debug panel existed.
        settings
            .shortcuts
            .retain(|binding| binding.action != ShortcutAction::WindowDebug);
        settings.unbind(ShortcutAction::WindowSamples);
        settings.normalize();
        assert_eq!(
            settings.keys_for(ShortcutAction::WindowDebug),
            "Ctrl+Shift+D"
        );
        // A deliberate unbind keeps its (empty) entry and stays unbound.
        assert!(settings.keys_for(ShortcutAction::WindowSamples).is_empty());
    }

    #[test]
    fn plugin_roots_join_every_format_list() {
        let mut settings = Settings::default();
        settings.vst_paths.push("C:/vst".to_owned());
        settings.vst3_paths.push("C:/vst3".to_owned());
        settings.clap_paths.push(String::new());
        settings.clap_paths.push("C:/clap".to_owned());
        assert_eq!(
            settings.plugin_roots(),
            vec!["C:/vst", "C:/vst3", "C:/clap"]
        );
    }

    #[test]
    fn settings_round_trip_through_json() {
        let mut settings = Settings {
            parameter_smoothing: false,
            smoothing_ms: 30.0,
            ..Settings::default()
        };
        settings.bind(ShortcutAction::ToolPen, "Ctrl+1");
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(!restored.parameter_smoothing);
        assert!((restored.smoothing_ms - 30.0).abs() < f32::EPSILON);
        assert_eq!(restored.keys_for(ShortcutAction::ToolPen), "Ctrl+1");
    }
}
