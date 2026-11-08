//! reafl binary entry point.
//!
//! The editor itself lives in the `reafl-app` crate so that the windows,
//! widgets and engine pieces can be built and tested independently of the
//! executable. This file only starts the editor.

/// Runs the reafl editor and returns the windowing result.
fn main() -> eframe::Result<()> {
    reafl_app::run()
}
