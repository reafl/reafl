//! The windows and panels of the editor.
//!
//! Each module owns one view and holds nothing but the `PulseForgeApp` methods
//! that draw it, so a view can be read - or replaced - on its own. The shared
//! state stays in [`crate::PulseForgeApp`]; the widgets come from
//! `pulseforge-widgets`.

pub mod arrangement;
pub mod automation;
pub mod automation_formula;
pub mod automation_lane;
pub mod automation_target;
pub mod automation_tracks;
pub mod clip_windows;
pub mod mixer;
pub mod panels;
pub mod piano;
pub mod piano_gestures;
pub mod piano_input;
pub mod piano_velocity;
pub mod plugin_effect;
pub mod plugin_hosted;
pub mod plugin_instrument;
pub mod plugin_windows;
pub mod plugins;
pub mod rack;
pub mod settings;
pub mod timeline;
pub mod timeline_input;
pub mod timeline_paint;
pub mod timeline_rows;
pub mod toolbar;
