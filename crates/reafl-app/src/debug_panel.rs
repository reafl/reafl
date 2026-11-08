//! The debug and profiler window.

use crate::*;

impl ReaflApp {
    /// Draws the debug/profiler window and hands it the live engine numbers.
    ///
    /// The panel is a separate crate so the DAW only owns the state that comes
    /// from the engine; everything else (sampling, aggregation, drawing) lives
    /// in `reafl-profiling`.
    pub(crate) fn render_debug_panel(&mut self, context: &egui::Context) {
        let (sample_rate, block_frames, dsp_load) = match &self.audio {
            Some(audio) => (audio.sample_rate, audio.block_frames(), audio.load()),
            None => (0, 0, 0.0),
        };
        let mut world = DebugWorld {
            profiler: reafl_profiling::profiler().map(std::convert::AsRef::as_ref),
            system: Some(&self.system_monitor),
            plugins: &mut self.plugin_debug,
            audio: AudioDebug {
                sample_rate,
                block_frames,
                dsp_load,
                playing: self.playing,
            },
        };
        self.debug_panel.show(context, &mut world);
        self.debug_summary = if self.debug_panel.is_open() {
            reafl_profiling::ui::summary_line(&world)
        } else {
            String::new()
        };
    }
}
