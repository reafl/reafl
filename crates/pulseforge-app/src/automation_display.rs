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
        let mut normalized: Option<f32> = None;
        let mut relative = 0.0;
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
                    relative += value;
                    continue;
                }
                normalized = Some(match normalized {
                    Some(total) => total + value - track.points[0].value,
                    None => value,
                });
                continue;
            }
            let multiplier = if track.modulation_points.is_empty() {
                1.0
            } else {
                Project::evaluate_curve(&track.modulation_points, position).clamp(0.0, 1.0)
            };
            let source_value = automation_source_preview(
                &resolved_lane_source(
                    &self.project,
                    track_id,
                    position,
                    &lane_driven_parameters(&self.project, track_id),
                )
                .unwrap_or_else(|| track.source.clone()),
                target,
                &self.project.lanes,
                &self.project.patterns,
                bpm,
                track.start_step,
                position,
            );
            let contribution = base + (source_value - base) * multiplier;
            touched = true;
            normalized =
                Some(normalized.unwrap_or(base_normalized) + contribution - base_normalized);
        }
        if !touched {
            return None;
        }
        Some(
            target.denormalize_value(
                (normalized.unwrap_or(base_normalized) + relative).clamp(0.0, 1.0),
            ),
        )
    }
}
