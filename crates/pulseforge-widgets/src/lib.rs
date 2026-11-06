//! Widgets and helpers shared by every PulseForge window.
//!
//! Nothing here knows about the editor: a widget is handed a value, an optional
//! automation target and a size, and it draws itself. That keeps the drawing
//! code reusable from any window and keeps the application crate free of
//! painting details.
//!
//! - [`anim`] eases scalars toward a target for the explicit redraw model.
//! - [`context_menu`] models right-click menus and their actions as data, so a
//!   window only decides *what* a menu offers and never *how* it is drawn.
//! - [`platform`] holds the few OS calls egui does not expose.
//! - [`theme`] carries the colour and sizing tokens, [`icons`] paints the vector
//!   glyphs, and [`knob`], [`fader`], [`meter`] and [`power`] are the controls.
//! - [`control`] is the one control type every window can use, and [`shape`]
//!   draws the points and curves - lanes, EQ responses and routing cables - that
//!   those windows share.
//!
//! Modules that are fully documented enable `#![warn(missing_docs)]` themselves;
//! the rest of the crate is documented gradually.

pub mod anim;
pub mod automation_bridge;
pub mod context_menu;
pub mod control;
pub mod effect_parameters;
pub mod eq_curve;
pub mod fader;
pub mod fader_math;
pub mod icons;
pub mod knob;
pub mod lane_slider;
pub mod meter;
pub mod mixer_strip;
pub mod platform;
pub mod power;
pub mod presets;
pub mod shape;
pub mod theme;
pub mod time_edit;
pub mod tooltip;
