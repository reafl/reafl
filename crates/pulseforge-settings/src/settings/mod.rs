//! Persistent application settings.
//!
//! Everything the settings window edits lives here and is stored in
//! `settings.json`. Keyboard bindings are data, not code: [`ShortcutAction`]
//! lists every command, and [`Settings::bind`] assigns a chord to one, so a new
//! command only needs an enum variant plus a handler.
//!
//! The value types live in their own modules: [`shortcuts`] holds the bindable
//! commands and chords, [`appearance`] the theme and tooltip options.

mod appearance;
mod shortcuts;

#[cfg(test)]
mod tests;

pub use appearance::*;
pub use shortcuts::*;

use std::fs;

use serde::{Deserialize, Serialize};

/// File name used inside the working directory.
pub const SETTINGS_FILE: &str = "settings.json";

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
        self.zoom = self.zoom.clamp(0.2, 3.0);
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
