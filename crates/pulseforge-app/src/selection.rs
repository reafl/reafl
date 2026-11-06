//! What is selected and how the selection grows.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn selected_note_count(&self) -> usize {
        let primary = usize::from(matches!(self.selection, Some(Selection::Note { .. })));
        primary
            + self
                .extra_selection
                .iter()
                .filter(|item| matches!(item, Selection::Note { .. }))
                .count()
    }

    pub(crate) fn select_all_notes(&mut self) {
        let all = self.note_order();
        let mut iter = all.into_iter();
        self.selection = iter.next();
        self.extra_selection = iter.collect();
        self.selection_anchor = self.selection;
    }

    /// Step count shown in the piano roll: covers the pattern length and every
    /// note, so shortening a pattern never hides notes that sit beyond it.
    pub(crate) fn piano_visible_steps(&self) -> u64 {
        let pattern = self.project.patterns.get(self.selected_pattern);
        let pattern_length = pattern.map_or(VISIBLE_PIANO_STEPS, |pattern| pattern.length_steps);
        let note_end = pattern.map_or(0, |pattern| {
            pattern
                .notes
                .iter()
                .map(|note| note.start_step.saturating_add(note.duration_steps))
                .max()
                .unwrap_or(0)
        });
        pattern_length
            .max(note_end.saturating_add(1))
            .clamp(VISIBLE_PIANO_STEPS, 4096)
    }

    /// Window title for a plug-in window: uses the hosted plug-in name when the
    /// instrument is backed by an external bundle.
    pub(crate) fn instrument_window_title(&self, instrument_id: usize) -> String {
        let Some(instrument) = self.project.instruments.get(instrument_id) else {
            return "Plug-in".to_owned();
        };
        if instrument.synth_kind == SynthKind::External && !instrument.plugin_path.is_empty() {
            return format!("{} \u{00B7} Hosted plug-in", instrument.name);
        }
        format!(
            "{} \u{00B7} {}",
            instrument.name,
            synth_kind_label(instrument.synth_kind)
        )
    }
}

impl PulseForgeApp {
    /// (shift, ctrl) modifiers for selection gestures.
    pub(crate) fn selection_modifiers(&self) -> (bool, bool) {
        let (input_shift, input_ctrl) = (self.mod_shift, self.mod_ctrl);
        (input_shift, input_ctrl)
    }

    /// True when `item` is part of the current selection.
    pub(crate) fn is_selected(&self, item: Selection) -> bool {
        self.selection == Some(item) || self.extra_selection.contains(&item)
    }

    /// Replaces the selection, or extends it when shift/ctrl is held.
    pub(crate) fn select_item(&mut self, item: Selection, shift: bool, ctrl: bool) {
        if shift {
            // Shift always toggles membership of the clicked item.
            if self.is_selected(item) {
                self.extra_selection.retain(|existing| *existing != item);
                if self.selection == Some(item) {
                    // Promote another selected item so the selection never
                    // points at something that was just removed.
                    self.selection = self.extra_selection.first().copied();
                    if self.selection.is_some() {
                        self.extra_selection.remove(0);
                    }
                }
                if self.selection.is_none() {
                    self.selection_anchor = None;
                }
            } else if self.selection.is_none() {
                self.selection = Some(item);
                self.selection_anchor = Some(item);
            } else {
                self.extra_selection.push(item);
                self.selection_anchor.get_or_insert(item);
            }
            return;
        }
        if ctrl {
            // Ctrl+click extends to a range from the anchor. The range is built
            // from the concrete entries of the same kind, so clips and channels
            // no longer resolve to notes.
            if let Some(anchor) = self.selection_anchor
                && let Some(entries) = self.selection_between(anchor, item)
            {
                if self.selection.is_none() {
                    self.selection = Some(anchor);
                }
                self.extra_selection.clear();
                for entry in entries {
                    if self.selection != Some(entry) && !self.extra_selection.contains(&entry) {
                        self.extra_selection.push(entry);
                    }
                }
                return;
            }
            if self.selection.is_none() {
                self.selection = Some(item);
            } else if !self.is_selected(item) {
                self.extra_selection.push(item);
            }
            self.selection_anchor.get_or_insert(item);
            return;
        }
        self.selection = Some(item);
        self.extra_selection.clear();
        self.selection_anchor = Some(item);
    }

    /// Selects a piano note, extending the selection when requested.
    pub(crate) fn select_note(
        &mut self,
        _index: usize,
        instrument: usize,
        pitch: u8,
        start_step: u64,
        shift: bool,
        ctrl: bool,
    ) {
        let item = Selection::Note {
            pattern: self.selected_pattern,
            instrument,
            pitch,
            start_step,
        };
        self.selected_instrument = instrument;
        self.status = format!("Selected note {} at step {}.", note_name(pitch), start_step);
        self.select_item(item, shift, ctrl);
    }

    /// Every scale offered in the piano-roll toolbar.
    pub(crate) const SCALE_NAMES: [&'static str; 11] = [
        "Chromatic",
        "Major",
        "Minor",
        "Harmonic minor",
        "Dorian",
        "Phrygian",
        "Lydian",
        "Mixolydian",
        "Pentatonic major",
        "Pentatonic minor",
        "Blues",
    ];

    pub(crate) fn scale_contains(&self, pitch: u8) -> bool {
        scale_contains_pitch(self.settings.scale_root, &self.settings.scale_name, pitch)
    }

    /// Nearest pitch inside the scale; used when note snapping is on.
    pub(crate) fn snap_pitch_to_scale(&self, pitch: u8) -> u8 {
        if !self.settings.scale_snap {
            return pitch;
        }
        snap_pitch_into_scale(self.settings.scale_root, &self.settings.scale_name, pitch)
    }

    /// Moves every selected note (or the whole pattern when nothing is
    /// selected) by `semitones` and/or by `steps` snap units.
    pub(crate) fn shift_selected_notes(&mut self, semitones: i32, steps: i32, use_snap: bool) {
        let snap = if use_snap {
            self.project.snap_ticks.max(1) as i64
        } else {
            0
        };
        let step_delta = steps as i64 * snap;
        let selected: Vec<(u8, u64)> = self
            .extra_selection
            .iter()
            .chain(self.selection.iter())
            .filter_map(|item| match item {
                Selection::Note {
                    pitch, start_step, ..
                } => Some((*pitch, *start_step)),
                _ => None,
            })
            .collect();
        if semitones == 0 && step_delta == 0 {
            return;
        }
        self.push_history();
        let Some(pattern) = self.project.patterns.get_mut(self.selected_pattern) else {
            return;
        };
        let mut moved = 0;
        for note in &mut pattern.notes {
            let key = (note.pitch, note.start_step);
            if !selected.is_empty() && !selected.contains(&key) {
                continue;
            }
            let pitch = (i32::from(note.pitch) + semitones).clamp(0, 127) as u8;
            let step = if step_delta >= 0 {
                note.start_step.saturating_add(step_delta as u64)
            } else {
                note.start_step.saturating_sub((-step_delta) as u64)
            };
            note.pitch = pitch;
            note.start_step = step;
            moved += 1;
        }
        // Keep the selection pointing at the notes that just moved.
        let shift = |item: &mut Selection| {
            if let Selection::Note {
                pitch, start_step, ..
            } = item
            {
                *pitch = (i32::from(*pitch) + semitones).clamp(0, 127) as u8;
                *start_step = if step_delta >= 0 {
                    start_step.saturating_add(step_delta as u64)
                } else {
                    start_step.saturating_sub((-step_delta) as u64)
                };
            }
        };
        if let Some(item) = self.selection.as_mut() {
            shift(item);
        }
        for item in &mut self.extra_selection {
            shift(item);
        }
        if moved > 0 {
            self.status = format!("Moved {moved} note(s).");
            self.sync_project();
        }
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selection = None;
        self.extra_selection.clear();
        self.selection_anchor = None;
    }

    /// Ordered list of the notes in the current pattern, used for range select.
    pub(crate) fn note_order(&self) -> Vec<Selection> {
        self.project
            .patterns
            .get(self.selected_pattern)
            .map(|pattern| {
                let mut notes: Vec<_> = pattern.notes.iter().collect();
                notes.sort_by_key(|note| (note.start_step, note.pitch));
                notes
                    .into_iter()
                    .map(|note| Selection::Note {
                        pattern: self.selected_pattern,
                        instrument: note.instrument_id,
                        pitch: note.pitch,
                        start_step: note.start_step,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clip order across the arrangement, used for ctrl range selection.
    pub(crate) fn clip_order(&self) -> Vec<Selection> {
        let mut clips = Vec::new();
        for (lane, data) in self.project.lanes.iter().enumerate() {
            for (clip, item) in data.clips.iter().enumerate() {
                clips.push((item.start_step, lane, clip));
            }
        }
        clips.sort_unstable();
        clips
            .into_iter()
            .map(|(_, lane, clip)| Selection::Clip { lane, clip })
            .collect()
    }

    /// Every selection between two selections of the same kind, inclusive.
    ///
    /// Returning the entries rather than indices matters because the kinds have
    /// separate orderings (notes, clips, channels); an index taken from one list
    /// would address a different item in another.
    pub(crate) fn selection_between(
        &self,
        from: Selection,
        to: Selection,
    ) -> Option<Vec<Selection>> {
        let list = match (from, to) {
            (Selection::Note { .. }, Selection::Note { .. }) => self.note_order(),
            (Selection::Clip { .. }, Selection::Clip { .. }) => self.clip_order(),
            (Selection::MixerChannel(_), Selection::MixerChannel(_)) => self.channel_order(),
            (Selection::Instrument(_), Selection::Instrument(_)) => {
                (0..self.project.instruments.len())
                    .map(Selection::Instrument)
                    .collect()
            }
            _ => return None,
        };
        let start = list.iter().position(|item| *item == from)?;
        let end = list.iter().position(|item| *item == to)?;
        let (low, high) = (start.min(end), start.max(end));
        Some(list[low..=high].to_vec())
    }

    /// Mixer channels, master last, used for range selection in the mixer and
    /// the arrangement track headers.
    pub(crate) fn channel_order(&self) -> Vec<Selection> {
        (0..=self.project.lanes.len())
            .map(Selection::MixerChannel)
            .collect()
    }

    /// Selects an arrangement lane (and its mixer channel) honouring the
    /// current modifier keys.
    pub(crate) fn select_lane(&mut self, lane: usize) {
        self.select_channel(lane);
    }

    /// Selects a mixer channel, honouring shift (toggle) and ctrl (range).
    pub(crate) fn select_channel(&mut self, channel: usize) {
        let (shift, ctrl) = (self.mod_shift, self.mod_ctrl);
        if channel < self.project.lanes.len() {
            self.selected_lane = channel;
        }
        self.selected_mixer_channel = channel;
        self.select_item(Selection::MixerChannel(channel), shift, ctrl);
    }
}
