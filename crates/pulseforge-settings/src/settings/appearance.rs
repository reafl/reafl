//! Appearance: colour theme and tooltip behaviour.

use serde::{Deserialize, Serialize};

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
