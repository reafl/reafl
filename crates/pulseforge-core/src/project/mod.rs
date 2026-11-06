//! The project model and the operations on it.
//!
//! [`Project`] is the whole saved document: patterns, arrangement lanes,
//! instruments, mixer routing and automation. It is split into the data
//! definitions in `model` and the behaviour in `behaviour`, and both are
//! re-exported here so callers keep writing `project::Project`.

mod behaviour;
mod model;

#[cfg(test)]
mod tests;

pub use behaviour::*;
pub use model::*;
