//! Step-sequencer and pattern edits.

use super::*;

impl Project {
    /// Adds or removes the step-sequencer note for `instrument_id` at `step`.
    ///
    /// Step-sequencer edits are stored as ordinary C5 piano-roll notes, so both
    /// editors show and edit the same data.
    pub fn set_step_note(&mut self, instrument_id: usize, step: u64, enabled: bool) {
        self.set_step_note_on_pattern(0, instrument_id, step, enabled);
    }

    /// Step-sequencer edit for a specific pattern.
    ///
    /// # Parameters
    ///
    /// - `pattern_id`: pattern to edit.
    /// - `instrument_id`: instrument whose step is changed.
    /// - `step`: step index inside the pattern.
    /// - `enabled`: `true` to add the hit, `false` to remove it.
    pub fn set_step_note_on_pattern(
        &mut self,
        pattern_id: usize,
        instrument_id: usize,
        step: u64,
        enabled: bool,
    ) {
        let pitch = STEP_SEQUENCER_PITCH;
        let Some(pattern) = self.patterns.get_mut(pattern_id) else {
            return;
        };
        let existing = pattern.notes.iter().position(|note| {
            note.instrument_id == instrument_id && note.pitch == pitch && note.start_step == step
        });
        match (enabled, existing) {
            (true, None) => pattern.notes.push(PianoNote {
                instrument_id,
                pitch,
                start_step: step,
                duration_steps: 1,
                velocity: 100,
                ..PianoNote::default()
            }),
            (true, Some(_)) => {}
            (false, Some(index)) => {
                pattern.notes.remove(index);
            }
            (false, None) => {}
        }
        pattern
            .notes
            .sort_by_key(|note| (note.start_step, note.pitch, note.instrument_id));
    }

    /// True when a step-sequencer note exists for this instrument and step.
    pub fn has_step_note(&self, pattern_id: usize, instrument_id: usize, step: u64) -> bool {
        self.patterns.get(pattern_id).is_some_and(|pattern| {
            pattern.notes.iter().any(|note| {
                note.instrument_id == instrument_id
                    && note.pitch == STEP_SEQUENCER_PITCH
                    && note.start_step == step
            })
        })
    }

    /// Appends a new empty pattern and returns its index.
    pub fn add_pattern(&mut self) -> usize {
        let id = self.patterns.len();
        self.patterns.push(Pattern {
            name: format!("Pattern {}", id + 1),
            ..Pattern::default()
        });
        id
    }

    /// Copies a clip onto the same lane, immediately after it.
    ///
    /// # Returns
    ///
    /// The index of the new clip, or `None` when the source clip is missing.
    pub fn duplicate_clip_after(&mut self, lane_id: usize, clip_index: usize) -> Option<usize> {
        let lane = self.lanes.get_mut(lane_id)?;
        let source = lane.clips.get(clip_index)?.clone();
        let start_step = source.start_step.saturating_add(source.length_steps.max(1));
        let insertion_index = lane
            .clips
            .iter()
            .take_while(|clip| clip.start_step <= start_step)
            .count();
        lane.clips.insert(
            insertion_index,
            ArrangementClip {
                start_step,
                length_steps: source.length_steps.max(1),
                source: source.source,
                offset_steps: 0,
                ..ArrangementClip::default()
            },
        );
        Some(insertion_index)
    }

    /// Copies the clip's pattern so edits to it no longer affect other clips
    /// that shared it.
    ///
    /// # Returns
    ///
    /// The index of the new pattern, or `None` for an audio clip.
    pub fn make_clip_pattern_unique(&mut self, lane_id: usize, clip_index: usize) -> Option<usize> {
        let pattern_id = match &self.lanes.get(lane_id)?.clips.get(clip_index)?.source {
            ClipSource::Pattern { pattern_id } => *pattern_id,
            ClipSource::Audio { .. } => return None,
        };
        let mut pattern = self.patterns.get(pattern_id)?.clone();
        let unique_id = self.patterns.len();
        pattern.name = format!("{} (unique {})", pattern.name, unique_id + 1);
        self.patterns.push(pattern);
        self.lanes[lane_id].clips[clip_index].source = ClipSource::Pattern {
            pattern_id: unique_id,
        };
        Some(unique_id)
    }
}
