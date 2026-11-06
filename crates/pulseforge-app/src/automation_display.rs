//! The values a lane currently drives, for the widgets.

use crate::*;

impl PulseForgeApp {
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
        let base = self.project.parameter_value(target)?;
        let base_normalized = target.normalize_value(base);
        // The arithmetic mirrors the engine's `automated_value`: drawn curves
        // replace or offset the value, while generated sources contribute a
        // delta from their own centre. Keeping the two in step is what makes a
        // knob show what is actually heard for every source kind.
        let mut curve_total: Option<f32> = None;
        let mut relative_total = 0.0;
        let mut additive = 0.0;
        let mut touched = false;
        for (track_id, track) in self.project.automation_tracks.iter().enumerate() {
            if !track.enabled
                || track.target != target
                || position < track.start_step as f64
                || position >= track.start_step.saturating_add(track.length_steps) as f64
            {
                continue;
            }
            if track.source.edits_points() {
                if track.points.is_empty() {
                    continue;
                }
                touched = true;
                let value = Project::evaluate_curve(&track.points, position);
                if track.source.is_relative() {
                    relative_total += value;
                    continue;
                }
                match &mut curve_total {
                    Some(total) => *total += value - track.points[0].value,
                    None => curve_total = Some(value),
                }
                continue;
            }
            let multiplier = if track.modulation_points.is_empty() {
                1.0
            } else {
                Project::evaluate_curve(&track.modulation_points, position).clamp(0.0, 1.0)
            };
            // The lane's own automation is resolved first, so an LFO whose rate
            // is driven by a peak lane moves the knob at that rate.
            let resolved = resolved_lane_source(
                &self.project,
                track_id,
                position,
                &lane_driven_parameters(&self.project, track_id),
            )
            .unwrap_or_else(|| track.source.clone());
            let centre = resolved.base().unwrap_or(0.0);
            // Only a peak lane needs the engine's live level; everything else is
            // derived from the project, so the lock is skipped for them.
            let peak_level = if matches!(resolved, project::AutomationSource::Peak { .. }) {
                self.peak_level(track_id)
            } else {
                0.0
            };
            let raw = automation_source_preview(
                &resolved,
                target,
                &self.project.lanes,
                &self.project.patterns,
                bpm,
                track.start_step,
                position,
                peak_level,
            );
            additive += centre + (raw - centre) * multiplier - base_normalized;
            touched = true;
        }
        if !touched {
            return None;
        }
        Some(target.denormalize_value(
            (curve_total.unwrap_or(base_normalized) + relative_total + additive).clamp(0.0, 1.0),
        ))
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
