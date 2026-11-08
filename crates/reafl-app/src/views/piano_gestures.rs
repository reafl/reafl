//! Piano-roll gestures and the note edits they perform.

use crate::*;

impl ReaflApp {
    /// Handles a press in the piano grid, starting the gesture the current tool
    /// implies.
    ///
    /// # Parameters
    ///
    /// - `tick`: pressed position in sixteenths from the clip start.
    /// - `pitch`: pitch under the pointer.
    /// - `hit_pitch`: pitch of the note that was hit, if any.
    /// - `pointer`: pointer position in screen space.
    /// - `grid_rect`: rectangle of the grid.
    /// - `step_width`: width of one step in points.
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
        let track = self.selected_instrument;
        let clip_index = self.resolve_active_clip().unwrap_or(0);
        self.push_history();
        match self.piano_tool {
            PianoTool::Pen => {
                let hit = {
                    let notes = self.active_notes();
                    notes.iter().enumerate().find(|(_, note)| {
                        note.pitch == hit_pitch
                            && tick >= note.start_tick()
                            && tick < note.start_tick().saturating_add(note.duration_ticks())
                    })
                };
                if let Some((index, note)) = hit.map(|(index, note)| (index, *note)) {
                    self.selection = Some(Selection::Note {
                        track,
                        clip: clip_index,
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
                    let default_duration = self.note_length_steps.max(1).saturating_mul(16);
                    let mut note = ClipNote {
                        pitch,
                        velocity: self.settings.default_velocity,
                        duration_steps: 1,
                        repeat_count: self.note_repeat_count,
                        repeat_ticks: self.note_repeat_ticks,
                        repeat_decay: self.note_repeat_decay,
                        ..ClipNote::default()
                    };
                    note.set_start_tick(tick);
                    note.set_duration_ticks(default_duration);
                    let note_index = match self.active_notes_mut() {
                        Some(notes) => {
                            notes.push(note);
                            notes.len() - 1
                        }
                        None => return,
                    };
                    self.selection = Some(Selection::Note {
                        track,
                        clip: clip_index,
                        pitch,
                        start_step: tick / 16,
                    });
                    self.piano_gesture = Some(PianoGesture::Draw {
                        pitch,
                        start_tick: tick,
                        note_index,
                        default_duration_ticks: default_duration,
                    });
                }
            }
            PianoTool::Brush => {
                // The brush paints notes at the hovered pitch, like FL's paint
                // tool, instead of toggling step-sequencer rows.
                let existing = self
                    .active_notes()
                    .iter()
                    .position(|note| note.pitch == pitch && note.start_step == step);
                let note_index = match existing {
                    Some(index) => index,
                    None => {
                        let mut note = ClipNote {
                            pitch,
                            velocity: self.settings.default_velocity,
                            duration_steps: 1,
                            repeat_count: self.note_repeat_count,
                            repeat_ticks: self.note_repeat_ticks,
                            repeat_decay: self.note_repeat_decay,
                            ..ClipNote::default()
                        };
                        note.set_start_tick(tick);
                        match self.active_notes_mut() {
                            Some(notes) => {
                                notes.push(note);
                                notes.len() - 1
                            }
                            None => return,
                        }
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
                let hit = self.active_notes().iter().enumerate().find(|(_, note)| {
                    note.pitch == hit_pitch
                        && tick >= note.start_tick()
                        && tick < note.start_tick().saturating_add(note.duration_ticks())
                });
                let additive = self.selection_modifiers();
                match hit.map(|(index, note)| (index, *note)) {
                    Some((_index, note)) => {
                        self.select_note(
                            0,
                            track,
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
    }

    /// Extends the running gesture to a new grid position.
    ///
    /// # Parameters
    ///
    /// - `tick`: pointer position in sixteenths.
    /// - `pitch`: pitch under the pointer.
    /// - `_pointer`: pointer position in screen space, unused by the edits.
    pub(crate) fn update_piano_gesture(&mut self, tick: u64, pitch: u8, _pointer: egui::Pos2) {
        let step = tick / 16;
        match self.piano_gesture {
            Some(PianoGesture::Draw {
                pitch: draw_pitch,
                start_tick,
                note_index,
                default_duration_ticks,
            }) => {
                if let Some(note) = self
                    .active_notes_mut()
                    .and_then(|notes| notes.get_mut(note_index))
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
                if let Some(note) = self
                    .active_notes_mut()
                    .and_then(|notes| notes.get_mut(note_index))
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
                if let Some(note) = self
                    .active_notes_mut()
                    .and_then(|notes| notes.get_mut(note_index))
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
                if let Some(note) = self
                    .active_notes_mut()
                    .and_then(|notes| notes.get_mut(note_index))
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
        let dragged = note_index.and_then(|index| {
            self.active_notes_mut()
                .and_then(|notes| notes.get(index))
                .copied()
        });
        if let Some(note) = dragged {
            self.selection = Some(Selection::Note {
                track: self.selected_instrument,
                clip: self.resolve_active_clip().unwrap_or(0),
                pitch: note.pitch,
                start_step: note.start_step,
            });
        }
    }

    /// Commits the running gesture: joins notes and re-sorts the clip.
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
            if let Some(notes) = self.active_notes_mut() {
                notes.sort_by_key(|note| (note.start_step, note.pitch));
            }
            self.sync_project();
        }
        self.piano_gesture = None;
    }

    /// Removes the note under the pointer.
    ///
    /// # Parameters
    ///
    /// - `tick`: position in sixteenths.
    /// - `pitch`: pitch to erase.
    pub(crate) fn remove_note_at(&mut self, tick: u64, pitch: u8) {
        let clip_index = self.resolve_active_clip().unwrap_or(0);
        let track = self.selected_instrument;
        let selection = self.selection;
        let Some(notes) = self.active_notes_mut() else {
            return;
        };
        let selected_note_was_removed = notes.iter().any(|note| {
            note.pitch == pitch
                && tick >= note.start_tick()
                && tick < note.start_tick().saturating_add(note.duration_ticks())
                && selection
                    == Some(Selection::Note {
                        track,
                        clip: clip_index,
                        pitch: note.pitch,
                        start_step: note.start_step,
                    })
        });
        let before = notes.len();
        notes.retain(|note| {
            !(note.pitch == pitch
                && tick >= note.start_tick()
                && tick < note.start_tick().saturating_add(note.duration_ticks()))
        });
        let removed = notes.len() != before;
        if removed {
            if selected_note_was_removed {
                self.selection = None;
            }
            self.sync_project();
        }
    }

    /// Splits the note under the pointer in two.
    ///
    /// # Parameters
    ///
    /// - `tick`: split position in sixteenths.
    /// - `pitch`: pitch to cut.
    pub(crate) fn cut_note_at(&mut self, tick: u64, pitch: u8) {
        let Some(notes) = self.active_notes_mut() else {
            return;
        };
        let Some(index) = notes.iter().position(|note| {
            note.pitch == pitch
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

    /// Merges every note of `pitch` between two steps into one.
    ///
    /// # Parameters
    ///
    /// - `pitch`: pitch to merge.
    /// - `start`: first tick of the range.
    /// - `end`: last tick of the range.
    pub(crate) fn join_notes(&mut self, pitch: u8, start: u64, end: u64) {
        let Some(notes) = self.active_notes_mut() else {
            return;
        };
        let mut matching: Vec<_> = notes
            .iter()
            .copied()
            .filter(|note| {
                note.pitch == pitch
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
            !(note.pitch == pitch
                && note.start_tick() <= end
                && note.start_tick().saturating_add(note.duration_ticks()) >= start)
        });
        let mut merged = template;
        merged.set_start_tick(merged_start);
        merged.set_duration_ticks(merged_end.saturating_sub(merged_start).max(1));
        notes.push(merged);
        let track = self.selected_instrument;
        let clip = self.resolve_active_clip().unwrap_or(0);
        self.selection = Some(Selection::Note {
            track,
            clip,
            pitch: template.pitch,
            start_step: merged_start / 16,
        });
    }
}
