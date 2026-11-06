//! The windows and panels of the editor.
//!
//! Each module owns one view and holds nothing but the `PulseForgeApp` methods
//! that draw it, so a view can be read - or replaced - on its own. The shared
//! state stays in [`crate::PulseForgeApp`]; the widgets come from
//! `pulseforge-widgets`.

pub mod arrangement;
pub mod automation;
pub mod mixer;
pub mod panels;
pub mod piano;
pub mod plugins;
pub mod rack;
pub mod settings;
pub mod toolbar;
