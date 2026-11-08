//! Key bindings: the bindable commands and the chords assigned to them.

use serde::{Deserialize, Serialize};

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
