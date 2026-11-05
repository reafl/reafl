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

pub mod anim;
pub mod context_menu;
pub mod icons;
pub mod platform;
pub mod presets;
pub mod theme;
pub mod tooltip;
