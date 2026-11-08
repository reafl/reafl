//! Configuration for reafl: what the user chose on the command line and
//! what persists between sessions.
//!
//! Both halves are plain data. [`settings::Settings`] is serialised to
//! `settings.json`, [`cli::Cli`] is parsed by `clap`, and neither knows about
//! egui, so they can be tested - and reused by tools - without a window.

pub mod cli;
pub mod settings;
