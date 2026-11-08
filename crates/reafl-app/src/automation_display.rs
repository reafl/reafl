//! The values a lane currently drives, for the widgets.

use crate::*;

impl ReaflApp {
    /// Current automated values for every automated parameter, used to show
    /// automation movement on knobs while transport is running.
    pub(crate) fn live_automated_values(&self) -> Vec<(ParameterTarget, f32)> {
        let Some(audio) = &self.audio else {
            return Vec::new();
        };
        // Also evaluated while stopped, so an automated control sits where the
        // playhead says it should instead of showing the stored value.
        let position = audio
            .playhead_steps
            .load(std::sync::atomic::Ordering::Relaxed) as f64;
        let bpm = self.project.bpm;
        let mut targets: Vec<ParameterTarget> = Vec::new();
        for track in &self.project.automation_tracks {
            if track.enabled
                && !targets.contains(&track.target)
                && position >= track.start_step as f64
                && position < track.start_step.saturating_add(track.length_steps) as f64
            {
                targets.push(track.target);
            }
        }
        let mut values = Vec::with_capacity(targets.len());
        for target in targets {
            if let Some(value) = self.automated_display_value(target, position, bpm) {
                values.push((target, value));
            }
        }
        values
    }

    pub(crate) fn automated_display_value(
        &self,
        target: ParameterTarget,
        position: f64,
        bpm: f32,
    ) -> Option<f32> {
        // The shared evaluator keeps the display in step with the engine, for
        // every source kind and every nesting of lane automation.
        automated_value_at(&self.project, target, position, bpm, &|track_id| {
            self.peak_level(track_id)
        })
    }

    /// Followed level of a peak lane, as published by the audio thread.
    ///
    /// # Parameters
    ///
    /// - `track_id`: automation lane index.
    ///
    /// # Returns
    ///
    /// The follower level in 0..=1, or `0.0` while no engine is running.
    pub(crate) fn peak_level(&self, track_id: usize) -> f32 {
        self.audio
            .as_ref()
            .and_then(|audio| audio.peak_level(track_id))
            .unwrap_or(0.0)
    }
}
