//! Piano-roll gestures and the note edits they perform.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn start_piano_gesture(
        &mut self,
        tick: u64,
        pitch: u8,
        hit_pitch: u8,
        pointer: egui::Pos2,
        grid_rect: egui::Rect,
        step_width: f32,
    ) {
        let step = tick / 16;
        self.push_history();
        let pattern = &mut self.project.patterns[self.selected_pattern];
        match self.piano_tool {
            PianoTool::Pen => {
                if let Some((index, note)) = pattern.notes.iter().enumerate().find(|(_, note)| {
                    note.instrument_id == self.selected_instrument
                        && note.pitch == hit_pitch
                        && tick >= note.start_tick()
                        && tick < note.start_tick().saturating_add(note.duration_ticks())
                }) {
                    self.selection = Some(Selection::Note {
                        pattern: self.selected_pattern,
                        instrument: note.instrument_id,
                        pitch: note.pitch,
                        start_step: note.start_step,
                    });
                    let start_tick = note.start_tick();
                    let end_tick = start_tick.saturating_add(note.duration_ticks());
                    let end_x = grid_rect.left()
                        + (end_tick as f32 / 16.0 - self.piano_pan_steps as f32) * step_width;
                    let start_x = grid_rect.left()
                        + (start_tick as f32 / 16.0 - self.piano_pan_steps as f32) * step_width;
                    // 7 px edge zones: left resizes the start, right the end.
                    const EDGE: f32 = 7.0;
                    if (pointer.x - end_x).abs() <= EDGE {
                        self.piano_gesture = Some(PianoGesture::Resize {
                            note_index: index,
                            origin_duration_ticks: note.duration_ticks(),
                            pointer_tick: tick,
                        });
                    } else if (pointer.x - start_x).abs() <= EDGE
                        && start_tick >= self.piano_pan_steps.saturating_mul(16)
                    {
                        self.piano_gesture = Some(PianoGesture::ResizeStart {
                            note_index: index,
                            origin_start_tick: start_tick,
                            origin_duration_ticks: note.duration_ticks(),
                            pointer_tick: tick,
                        });
                    } else {
                        self.piano_gesture = Some(PianoGesture::Move {
                            note_index: index,
                            origin_tick: start_tick,
                            origin_pitch: note.pitch,
                            pointer_tick: tick,
                            pointer_pitch: pitch,
                        });
                    }
                } else {
                    let note_index = pattern.notes.len();
                    let mut note = PianoNote {
                        instrument_id: self.selected_instrument,
                        duration_steps: 1,
                        pitch,
                        velocity: self.settings.default_velocity,
                        repeat_count: self.note_repeat_count,
                        repeat_ticks: self.note_repeat_ticks,
                        repeat_decay: self.note_repeat_decay,
                        ..PianoNote::default()
                    };
                    note.set_start_tick(tick);
                    note.set_duration_ticks(self.note_length_steps.max(1).saturating_mul(16));
                    pattern.notes.push(note);
                    self.selection = Some(Selection::Note {
                        pattern: self.selected_pattern,
                        instrument: self.selected_instrument,
                        pitch,
                        start_step: tick / 16,
                    });
                    self.piano_gesture = Some(PianoGesture::Draw {
                        pitch,
                        start_tick: tick,
                        note_index,
                        default_duration_ticks: self.note_length_steps.max(1).saturating_mul(16),
                    });
                }
            }
            PianoTool::Brush => {
                // The brush paints notes at the hovered pitch, like FL's paint
                // tool, instead of toggling step-sequencer rows.
                let exists = pattern.notes.iter().position(|note| {
                    note.instrument_id == self.selected_instrument
                        && note.pitch == pitch
                        && note.start_step == step
                });
                let note_index = match exists {
                    Some(index) => index,
                    None => {
                        let mut note = PianoNote {
                            instrument_id: self.selected_instrument,
                            pitch,
                            duration_steps: 1,
                            velocity: self.settings.default_velocity,
                            repeat_count: self.note_repeat_count,
                            repeat_ticks: self.note_repeat_ticks,
                            repeat_decay: self.note_repeat_decay,
                            ..PianoNote::default()
                        };
                        note.set_start_tick(tick);
                        pattern.notes.push(note);
                        pattern.notes.len() - 1
                    }
                };
                self.piano_gesture = Some(PianoGesture::Draw {
                    pitch,
                    start_tick: tick,
                    note_index,
                    default_duration_ticks: 16,
                });
            }
            PianoTool::Select => {
                // Clicking a note selects it, clicking empty space clears the
                // selection; shift and ctrl extend it.
                let hit = {
                    let notes = &self.project.patterns[self.selected_pattern].notes;
                    notes.iter().position(|note| {
                        note.instrument_id == self.selected_instrument
                            && note.pitch == hit_pitch
                            && tick >= note.start_tick()
                            && tick < note.start_tick().saturating_add(note.duration_ticks())
                    })
                };
                let additive = self.selection_modifiers();
                match hit {
                    Some(index) => {
                        let note = self.project.patterns[self.selected_pattern].notes[index];
                        self.select_note(
                            index,
                            note.instrument_id,
                            note.pitch,
                            note.start_step,
                            additive.0,
                            additive.1,
                        );
                    }
                    None => {
                        if !additive.0 && !additive.1 {
                            self.clear_selection();
                        }
                    }
                }
                // Ctrl+drag always starts a rubber band, even when the press
                // lands on a note.
                if additive.1 {
                    self.piano_box_origin = Some((tick, pitch));
                }
            }
            PianoTool::Join => {
                self.piano_gesture = Some(PianoGesture::Join {
                    pitch,
                    start_step: tick,
                    end_step: tick,
                });
            }
            PianoTool::Cut => self.cut_note_at(tick, pitch),
        }
        let _ = (pointer, grid_rect);
    }

    pub(crate) fn update_piano_gesture(&mut self, tick: u64, pitch: u8, _pointer: egui::Pos2) {
        let step = tick / 16;
        match self.piano_gesture {
            Some(PianoGesture::Draw {
                pitch: draw_pitch,
                start_tick,
                note_index,
                default_duration_ticks,
            }) => {
                if let Some(note) = self.project.patterns[self.selected_pattern]
                    .notes
                    .get_mut(note_index)
                {
                    let end_tick = tick.max(start_tick);
                    note.set_duration_ticks(if end_tick == start_tick {
                        default_duration_ticks
                    } else {
                        end_tick.saturating_sub(start_tick).max(1)
                    });
                    note.pitch = draw_pitch;
                }
            }
            Some(PianoGesture::Move {
                note_index,
                origin_tick,
                origin_pitch,
                pointer_tick,
                pointer_pitch,
            }) => {
                let delta_tick = tick as i128 - pointer_tick as i128;
                let delta_pitch = i16::from(pitch) - i16::from(pointer_pitch);
                if let Some(note) = self.project.patterns[self.selected_pattern]
                    .notes
                    .get_mut(note_index)
                {
                    note.set_start_tick((origin_tick as i128 + delta_tick).max(0) as u64);
                    note.pitch = (i16::from(origin_pitch) + delta_pitch).clamp(0, 127) as u8;
                }
            }
            Some(PianoGesture::Resize {
                note_index,
                origin_duration_ticks,
                pointer_tick,
            }) => {
                if let Some(note) = self.project.patterns[self.selected_pattern]
                    .notes
                    .get_mut(note_index)
                {
                    let delta = tick as i128 - pointer_tick as i128;
                    note.set_duration_ticks(
                        (origin_duration_ticks as i128 + delta).clamp(1, u64::MAX as i128) as u64,
                    );
                }
            }
            Some(PianoGesture::ResizeStart {
                note_index,
                origin_start_tick,
                origin_duration_ticks,
                pointer_tick,
            }) => {
                if let Some(note) = self.project.patterns[self.selected_pattern]
                    .notes
                    .get_mut(note_index)
                {
                    let (start_tick, duration_ticks) = resize_note_start(
                        origin_start_tick,
                        origin_duration_ticks,
                        pointer_tick,
                        tick,
                    );
                    note.set_start_tick(start_tick);
                    note.set_duration_ticks(duration_ticks);
                }
            }
            Some(PianoGesture::Join {
                pitch: join_pitch,
                start_step,
                ..
            }) => {
                self.piano_gesture = Some(PianoGesture::Join {
                    pitch: join_pitch,
                    start_step,
                    end_step: step,
                });
            }
            _ => {}
        }
        let note_index = match self.piano_gesture {
            Some(PianoGesture::Draw { note_index, .. })
            | Some(PianoGesture::Move { note_index, .. })
            | Some(PianoGesture::Resize { note_index, .. })
            | Some(PianoGesture::ResizeStart { note_index, .. }) => Some(note_index),
            _ => None,
        };
        if let Some(note) = note_index.and_then(|index| {
            self.project.patterns[self.selected_pattern]
                .notes
                .get(index)
                .copied()
        }) {
            self.selection = Some(Selection::Note {
                pattern: self.selected_pattern,
                instrument: note.instrument_id,
                pitch: note.pitch,
                start_step: note.start_step,
            });
        }
    }

    pub(crate) fn finish_piano_gesture(&mut self) {
        if let Some(PianoGesture::Join {
            pitch,
            start_step,
            end_step,
        }) = self.piano_gesture
        {
            self.join_notes(pitch, start_step.min(end_step), start_step.max(end_step));
        }
        if self.piano_gesture.is_some() {
            self.project.patterns[self.selected_pattern]
                .notes
                .sort_by_key(|note| (note.start_step, note.pitch, note.instrument_id));
            self.sync_project();
        }
        self.piano_gesture = None;
    }

    pub(crate) fn remove_note_at(&mut self, tick: u64, pitch: u8) {
        let notes = &mut self.project.patterns[self.selected_pattern].notes;
        let selected_note_was_removed = notes.iter().any(|note| {
            note.instrument_id == self.selected_instrument
                && note.pitch == pitch
                && tick >= note.start_tick()
                && tick < note.start_tick().saturating_add(note.duration_ticks())
                && self.selection
                    == Some(Selection::Note {
                        pattern: self.selected_pattern,
                        instrument: note.instrument_id,
                        pitch: note.pitch,
                        start_step: note.start_step,
                    })
        });
        let before = notes.len();
        notes.retain(|note| {
            !(note.instrument_id == self.selected_instrument
                && note.pitch == pitch
                && tick >= note.start_tick()
                && tick < note.start_tick().saturating_add(note.duration_ticks()))
        });
        if notes.len() != before {
            if selected_note_was_removed {
                self.selection = None;
            }
            self.sync_project();
        }
    }

    pub(crate) fn cut_note_at(&mut self, tick: u64, pitch: u8) {
        let notes = &mut self.project.patterns[self.selected_pattern].notes;
        let Some(index) = notes.iter().position(|note| {
            note.instrument_id == self.selected_instrument
                && note.pitch == pitch
                && tick > note.start_tick()
                && tick < note.start_tick().saturating_add(note.duration_ticks())
        }) else {
            return;
        };
        let original = notes[index];
        let first_duration = tick - original.start_tick();
        let second_duration = original.duration_ticks() - first_duration;
        notes[index].set_duration_ticks(first_duration);
        let mut second = original;
        second.set_start_tick(tick);
        second.set_duration_ticks(second_duration);
        notes.push(second);
        self.sync_project();
    }

    pub(crate) fn join_notes(&mut self, pitch: u8, start: u64, end: u64) {
        let notes = &mut self.project.patterns[self.selected_pattern].notes;
        let mut matching: Vec<_> = notes
            .iter()
            .copied()
            .filter(|note| {
                note.instrument_id == self.selected_instrument
                    && note.pitch == pitch
                    && note.start_tick() <= end
                    && note.start_tick().saturating_add(note.duration_ticks()) >= start
            })
            .collect();
        if matching.len() < 2 {
            return;
        }
        matching.sort_by_key(|note| note.start_tick());
        let merged_start = matching.first().unwrap().start_tick();
        let merged_end = matching
            .iter()
            .map(|note| note.start_tick().saturating_add(note.duration_ticks()))
            .max()
            .unwrap_or(merged_start + 16);
        let template = matching[0];
        notes.retain(|note| {
            !(note.instrument_id == self.selected_instrument
                && note.pitch == pitch
                && note.start_tick() <= end
                && note.start_tick().saturating_add(note.duration_ticks()) >= start)
        });
        let mut merged = template;
        merged.set_start_tick(merged_start);
        merged.set_duration_ticks(merged_end.saturating_sub(merged_start).max(1));
        notes.push(merged);
        self.selection = Some(Selection::Note {
            pattern: self.selected_pattern,
            instrument: template.instrument_id,
            pitch: template.pitch,
            start_step: merged_start / 16,
        });
    }
}

