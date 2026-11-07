//! The piano-roll note grid and its box-selection helpers.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn piano_grid(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let visible_steps = self.piano_visible_steps();
        let step_width = ((ui.available_width() - 54.0) / visible_steps as f32)
            .clamp(PIANO_MIN_STEP_WIDTH, PIANO_MAX_STEP_WIDTH)
            * self.settings.zoom;
        let row_height = (ui
            .available_height()
            .max(PIANO_MIN_ROW_HEIGHT * PIANO_ROWS as f32)
            / PIANO_ROWS as f32)
            .clamp(PIANO_MIN_ROW_HEIGHT, PIANO_MAX_ROW_HEIGHT);
        let width = visible_steps as f32 * step_width;
        let height = PIANO_ROWS as f32 * row_height;
        // The control lane is pinned under the grid; collapsing it hands the
        // space back to the notes.
        let lane_height = if self.control_lane_collapsed {
            0.0
        } else {
            VELOCITY_LANE_HEIGHT
        };
        let (ruler_rect, keys_rect, grid_rect, response, velocity_rect, velocity_response) = ui
            .vertical(|ui| {
                let (ruler_rect, _) =
                    ui.allocate_exact_size(egui::vec2(54.0 + width, 14.0), egui::Sense::hover());
                let (keys_rect, grid_rect, response) = ui
                    .horizontal(|ui| {
                        let (keys_rect, _) =
                            ui.allocate_exact_size(egui::vec2(54.0, height), egui::Sense::hover());
                        let (grid_rect, response) = ui.allocate_exact_size(
                            egui::vec2(width, height),
                            egui::Sense::click_and_drag(),
                        );
                        (keys_rect, grid_rect, response)
                    })
                    .inner;
                let (velocity_rect, velocity_response) = ui
                    .horizontal(|ui| {
                        ui.allocate_exact_size(egui::vec2(54.0, lane_height), egui::Sense::hover());
                        ui.allocate_exact_size(
                            egui::vec2(width, lane_height),
                            egui::Sense::click_and_drag(),
                        )
                    })
                    .inner;
                (
                    ruler_rect,
                    keys_rect,
                    grid_rect,
                    response,
                    velocity_rect,
                    velocity_response,
                )
            })
            .inner;
        let painter = ui.painter_at(grid_rect);
        let ruler_painter = ui.painter_at(ruler_rect);
        let absolute_start = self.piano_pan_steps;
        let visible_end = absolute_start.saturating_add(visible_steps);
        let pattern = &self.project.tracks[self.selected_clip];

        for row in 0..PIANO_ROWS {
            let pitch = PIANO_MIN_PITCH + (PIANO_ROWS - 1 - row) as u8;
            let y = grid_rect.top() + row as f32 * row_height;
            let rect = egui::Rect::from_min_size(
                egui::pos2(grid_rect.left(), y),
                egui::vec2(grid_rect.width(), row_height),
            );
            let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            // Rows outside the selected scale are dimmed, so the usable notes
            // are obvious at a glance.
            let in_scale = self.scale_contains(pitch);
            let base = if black {
                egui::Color32::from_gray(30)
            } else {
                egui::Color32::from_gray(38)
            };
            painter.rect_filled(
                rect,
                0.0,
                if in_scale {
                    base
                } else {
                    base.gamma_multiply(0.72)
                },
            );
            let key_rect = egui::Rect::from_min_size(
                egui::pos2(keys_rect.left(), y),
                egui::vec2(keys_rect.width(), row_height),
            );
            ui.painter().rect_filled(
                key_rect,
                1.0,
                if black {
                    egui::Color32::from_gray(55)
                } else {
                    egui::Color32::from_gray(192)
                },
            );
            // The scale root is marked on the keys, so the highlighted rows have
            // a visible reference note.
            if pitch % 12 == self.settings.scale_root % 12 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        key_rect.left_top(),
                        egui::vec2(3.0, key_rect.height()),
                    ),
                    0.0,
                    self.ui_accent(),
                );
            }
            if pitch % 12 == 0 {
                ui.painter().text(
                    key_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    note_name(pitch),
                    egui::FontId::proportional(10.0),
                    egui::Color32::BLACK,
                );
            }
        }
        for step in 0..=visible_steps {
            let x = grid_rect.left() + step as f32 * step_width;
            painter.line_segment(
                [
                    egui::pos2(x, grid_rect.top()),
                    egui::pos2(x, grid_rect.bottom()),
                ],
                egui::Stroke::new(
                    if (absolute_start + step) % 4 == 0 {
                        1.2
                    } else {
                        0.5_f32
                    },
                    if (absolute_start + step) % 4 == 0 {
                        egui::Color32::from_gray(100)
                    } else {
                        egui::Color32::from_gray(58)
                    },
                ),
            );
            if step < visible_steps {
                ruler_painter.text(
                    egui::pos2(x + step_width * 0.5, ruler_rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    (absolute_start + step).to_string(),
                    egui::FontId::proportional(9.0),
                    if (absolute_start + step) % 4 == 0 {
                        egui::Color32::from_gray(215)
                    } else {
                        egui::Color32::from_gray(140)
                    },
                );
            }
        }
        for note in pattern.notes.iter().filter(|note| {
            note.instrument_id == self.selected_instrument
                && note.start_step < visible_end
                && note.start_step.saturating_add(note.duration_steps) > absolute_start
                && note.pitch >= PIANO_MIN_PITCH
                && usize::from(note.pitch - PIANO_MIN_PITCH) < PIANO_ROWS
        }) {
            let row = PIANO_ROWS - 1 - usize::from(note.pitch - PIANO_MIN_PITCH);
            let note_start = note.start_tick() as f32 / 16.0;
            let note_end = note.start_tick().saturating_add(note.duration_ticks()) as f32 / 16.0;
            let x = grid_rect.left() + (note_start - absolute_start as f32) * step_width;
            let right = grid_rect.left() + (note_end - absolute_start as f32) * step_width;
            let rect = egui::Rect::from_min_max(
                egui::pos2(
                    x.max(grid_rect.left()) + 1.0,
                    grid_rect.top() + row as f32 * row_height + 2.0,
                ),
                egui::pos2(
                    right.min(grid_rect.right()) - 1.0,
                    grid_rect.top() + (row + 1) as f32 * row_height - 2.0,
                ),
            );
            painter.rect_filled(rect, 3.0, lane_color(self.selected_instrument));
            if self.is_selected(Selection::Note {
                track: self.selected_instrument,
                clip: self.selected_clip,
                pitch: note.pitch,
                start_step: note.start_step,
            }) {
                painter.rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2((rect.right() - 5.0).max(rect.left()), rect.top()),
                    rect.right_bottom(),
                ),
                2.0,
                egui::Color32::WHITE.gamma_multiply(0.35),
            );
            // Repeats are drawn as thin dividers so a repeated note shows where
            // each retrigger lands without leaving the piano roll.
            if note.repeat_count > 0 {
                let spacing = if note.repeat_ticks == 0 {
                    note.duration_ticks().max(1)
                } else {
                    u64::from(note.repeat_ticks) * 4
                };
                let marker = egui::Stroke::new(1.0_f32, egui::Color32::WHITE.gamma_multiply(0.6));
                for repeat in 1..=u64::from(note.repeat_count) {
                    let repeat_tick = note.start_tick() + spacing * repeat;
                    let marker_x = grid_rect.left()
                        + (repeat_tick as f32 / 16.0 - absolute_start as f32) * step_width;
                    if marker_x > rect.left() + 1.0 && marker_x < rect.right() - 1.0 {
                        painter.line_segment(
                            [
                                egui::pos2(marker_x, rect.top() + 2.0),
                                egui::pos2(marker_x, rect.bottom() - 2.0),
                            ],
                            marker,
                        );
                    }
                }
            }
        }

        self.handle_piano_input(grid_rect, response, context, step_width, row_height);
        if !self.control_lane_collapsed {
            self.velocity_lane(
                ui,
                context,
                velocity_rect,
                velocity_response,
                step_width,
                absolute_start,
            );
        }
        if self.playing {
            let position = self.playhead_step();
            let x = grid_rect.left() + (position - absolute_start as f64) as f32 * step_width;
            if x >= grid_rect.left() && x <= grid_rect.right() {
                painter.line_segment(
                    [
                        egui::pos2(x, grid_rect.top()),
                        egui::pos2(x, grid_rect.bottom()),
                    ],
                    egui::Stroke::new(1.6_f32, egui::Color32::from_rgb(255, 104, 79)),
                );
                ruler_painter.circle_filled(
                    egui::pos2(x, ruler_rect.center().y),
                    3.5,
                    egui::Color32::from_rgb(255, 104, 79),
                );
            }
        }
        // Dragging across the ruler selects every note that starts inside the
        // range. Unlike the arrangement loop this never affects playback.
        let ruler_response = ui.interact(
            ruler_rect,
            egui::Id::new("piano_ruler_select"),
            egui::Sense::click_and_drag(),
        );
        let ruler_step = |x: f32| -> u64 {
            let local = ((x - grid_rect.left()) / step_width).max(0.0);
            absolute_start.saturating_add(local as u64)
        };
        if ruler_response.drag_started() {
            if let Some(pointer) = context.input(|input| input.pointer.interact_pos()) {
                self.piano_select_start = Some(ruler_step(pointer.x));
            }
        }
        if ruler_response.dragged()
            && let (Some(start), Some(pointer)) = (
                self.piano_select_start,
                context.input(|input| input.pointer.interact_pos()),
            )
        {
            let end = ruler_step(pointer.x);
            self.piano_selection = Some((start.min(end), start.max(end).saturating_add(1)));
        }
        if ruler_response.drag_stopped()
            && let Some((start, end)) = self.piano_selection
        {
            self.select_notes_in_range(start, end);
            self.piano_select_start = None;
        }
        if let Some((start, end)) = self.piano_selection {
            let x0 = grid_rect.left() + (start.saturating_sub(absolute_start)) as f32 * step_width;
            let x1 = grid_rect.left() + (end.saturating_sub(absolute_start)) as f32 * step_width;
            ruler_painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x0, ruler_rect.top() + 1.0),
                    egui::pos2(x1.max(x0 + 2.0), ruler_rect.bottom() - 1.0),
                ),
                2.0,
                egui::Color32::from_rgb(112, 208, 190),
            );
        }
        // Shows the Ctrl+drag rubber band while it is being dragged.
        if let Some((origin_tick, origin_pitch)) = self.piano_box_origin
            && let Some(pointer) = context.input(|input| input.pointer.interact_pos())
        {
            let row_of = |pitch: u8| {
                let offset = pitch
                    .saturating_sub(PIANO_MIN_PITCH)
                    .min((PIANO_ROWS - 1) as u8);
                (PIANO_ROWS - 1 - usize::from(offset)) as f32
            };
            let x0 =
                grid_rect.left() + (origin_tick as f32 / 16.0 - absolute_start as f32) * step_width;
            let y0 = grid_rect.top() + row_of(origin_pitch) * row_height;
            let pointer_row = ((pointer.y - grid_rect.top()) / row_height)
                .floor()
                .clamp(0.0, (PIANO_ROWS - 1) as f32);
            let y1 = grid_rect.top() + (pointer_row + 1.0) * row_height;
            let band = egui::Rect::from_two_pos(
                egui::pos2(x0, y0),
                egui::pos2(pointer.x.clamp(grid_rect.left(), grid_rect.right()), y1),
            );
            if band.width() > 2.0 || band.height() > 2.0 {
                painter.rect_filled(
                    band,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(112, 208, 190, 40),
                );
            }
        }
        self.paint_tool_cursor(ui, context, grid_rect, step_width, row_height);
    }

    /// Selects every note whose start lies inside the rubber band.
    pub(crate) fn select_notes_in_box(
        &mut self,
        from_tick: u64,
        to_tick: u64,
        from_pitch: u8,
        to_pitch: u8,
    ) {
        let (low_tick, high_tick) = (from_tick.min(to_tick), from_tick.max(to_tick));
        let (low_pitch, high_pitch) = (from_pitch.min(to_pitch), from_pitch.max(to_pitch));
        let instrument = self.selected_instrument;
        let hits: Vec<Selection> = self.project.tracks[self.selected_clip]
            .notes
            .iter()
            .filter(|note| {
                // A note is selected when the band overlaps its body, not only
                // when it starts inside, which is what users expect from a box.
                let start = note.start_tick();
                let end = start.saturating_add(note.duration_ticks().max(1));
                note.instrument_id == instrument
                    && end > low_tick
                    && start <= high_tick
                    && note.pitch >= low_pitch
                    && note.pitch <= high_pitch
            })
            .map(|note| Selection::Note {
                track: self.selected_instrument,
                clip: self.selected_clip,
                pitch: note.pitch,
                start_step: note.start_step,
            })
            .collect();
        if let Some(first) = hits.first().copied() {
            self.selection = Some(first);
        }
        self.extra_selection = hits.iter().skip(1).copied().collect();
        self.selection_anchor = hits.first().copied();
        self.status = format!("{} note(s) in the box.", hits.len());
    }

    /// Selects every note starting inside `[start, end)` steps.
    pub(crate) fn select_notes_in_range(&mut self, start: u64, end: u64) {
        let Some(pattern) = self.project.tracks.get(self.selected_clip) else {
            return;
        };
        let hits: Vec<Selection> = pattern
            .notes
            .iter()
            .filter(|note| note.start_step >= start && note.start_step < end)
            .map(|note| Selection::Note {
                track: self.selected_instrument,
                clip: self.selected_clip,
                pitch: note.pitch,
                start_step: note.start_step,
            })
            .collect();
        self.selection = hits.first().copied();
        self.extra_selection = hits.iter().skip(1).copied().collect();
        self.selection_anchor = hits.first().copied();
        self.status = format!("Selected {} note(s) in the ruler range.", hits.len());
    }
}
