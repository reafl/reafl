//! The project data model, split by topic.
//!
//! Everything here is plain data with small helpers: [`params`] holds time and
//! modulation settings, [`piano`] the notes and the arpeggiator, [`device`] the
//! instrument and effect types, [`targets`] the automatable parameter addresses,
//! [`automation`] the lanes themselves, and [`arrangement`] the instruments,
//! clips and the [`Project`] document that ties them together. The behaviour
//! lives in [`crate::project::behaviour`].

mod arrangement;
mod automation;
mod device;
mod params;
mod piano;
mod targets;

pub use arrangement::*;
pub use automation::*;
pub use device::*;
pub use params::*;
pub use piano::*;
pub use targets::*;
