//! The piano roll, its gestures and note editing.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn render_piano_roll(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        profile_scope!("render_piano_roll");
        egui::Frame::NONE.inner_margin(4.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Pattern");
                let name = &self.project.patterns[self.selected_pattern].name;
                egui::ComboBox::from_id_salt("piano_pattern")
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for (id, pattern) in self.project.patterns.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_pattern, id, &pattern.name);
                        }
                    });
                ui.label("Instrument");
                egui::ComboBox::from_id_salt("piano_instrument")
                    .selected_text(&self.project.instruments[self.selected_instrument].name)
                    .show_ui(ui, |ui| {
                        for (id, instrument) in self.project.instruments.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_instrument,
                                id,
                                &instrument.name,
                            );
                        }
                    });
                ui.label(format!("Snap: {}", snap_label(self.project.snap_ticks)));
                ui.separator();
                // Scale highlighting and note snapping.
                ui.label("Scale");
                egui::ComboBox::from_id_salt("piano_scale")
                    .selected_text(&self.settings.scale_name)
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for name in Self::SCALE_NAMES {
                            ui.selectable_value(
                                &mut self.settings.scale_name,
                                name.to_owned(),
                                name,
                            );
                        }
                    });
                ui.label("Root");
                let mut root = self.settings.scale_root % 12;
                egui::ComboBox::from_id_salt("piano_scale_root")
                    .selected_text(note_name(root))
                    .width(58.0)
                    .show_ui(ui, |ui| {
                        for value in 0..12u8 {
                            ui.selectable_value(&mut root, value, note_name(value));
                        }
                    });
                self.settings.scale_root = root;
                ui.checkbox(&mut self.settings.scale_snap, "Snap to scale");
                ui.separator();
                ui.label("Length");
                ui.add(
                    egui::DragValue::new(&mut self.note_length_steps)
                        .range(1..=u64::MAX)
                        .speed(1.0),
                );
                ui.separator();
                ui.label("New notes");
                ui.add(
                    egui::DragValue::new(&mut self.note_repeat_count)
                        .range(0..=32)
                        .speed(0.1),
                )
                .on_hover_text("Extra triggers for newly drawn notes");
                ui.label("Every");
                ui.add(
                    egui::DragValue::new(&mut self.note_repeat_ticks)
                        .range(0..=64)
                        .speed(0.2),
                )
                .on_hover_text("Ticks between repeats (0 = half the note length)");
                ui.label("Decay");
                ui.add(
                    egui::DragValue::new(&mut self.note_repeat_decay)
                        .range(0..=126)
                        .speed(0.5),
                )
                .on_hover_text("Velocity lost per repeat");
                if ui
                    .button("Apply to selected")
                    .on_hover_text("Apply the repeat settings to the selected notes")
                    .clicked()
                {
                    self.apply_repeat_to_selection();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Control");
                for (lane, label, hint) in [
                    (
                        ControlLane::Velocity,
                        "Velocity",
                        "Velocity lane (Ctrl+Shift+V)",
                    ),
                    (
                        ControlLane::Repeats,
                        "Repeats",
                        "Repeats lane (Ctrl+Shift+R)",
                    ),
                ] {
                    if ui
                        .selectable_label(self.control_lane == lane, label)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        self.control_lane = lane;
                    }
                }
                if ui
                    .selectable_label(!self.control_lane_collapsed, "Lane")
                    .on_hover_text("Show or hide the control lane")
                    .clicked()
                {
                    self.control_lane_collapsed = !self.control_lane_collapsed;
                }
                ui.separator();
                let selected_notes = self.selected_note_count();
                ui.label(format!("Sel {selected_notes}"));
                if ui
                    .small_button("All")
                    .on_hover_text("Select every note")
                    .clicked()
                {
                    self.select_all_notes();
                }
                if ui.small_button("Clear").clicked() {
                    self.clear_selection();
                }
                if ui
                    .small_button("Range")
                    .on_hover_text("Ctrl+click two notes to select everything between them")
                    .clicked()
                {
                    self.status =
                        "Ctrl+click the first and last note to select the range.".to_owned();
                }
                ui.separator();
                tool_palette(ui, &mut self.piano_tool, true);
                ui.label("Right-drag deletes · Middle-drag pans · Alt disables snap");
            });
            ui.separator();
        });
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(ui, |ui| self.piano_grid(ui, context));
    }

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
                        ui.allocate_exact_size(
                            egui::vec2(54.0, lane_height),
                            egui::Sense::hover(),
                        );
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
        let pattern = &self.project.patterns[self.selected_pattern];

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
                pattern: self.selected_pattern,
                instrument: note.instrument_id,
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
                            [egui::pos2(marker_x, rect.top() + 2.0), egui::pos2(marker_x, rect.bottom() - 2.0)],
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
            let x0 = grid_rect.left()
                + (origin_tick as f32 / 16.0 - absolute_start as f32) * step_width;
            let y0 = grid_rect.top() + row_of(origin_pitch) * row_height;
            let pointer_row = ((pointer.y - grid_rect.top()) / row_height)
                .floor()
                .clamp(0.0, (PIANO_ROWS - 1) as f32);
            let y1 = grid_rect.top() + (pointer_row + 1.0) * row_height;
            let band = egui::Rect::from_two_pos(
                egui::pos2(x0, y0),
                egui::pos2(
                    pointer.x.clamp(grid_rect.left(), grid_rect.right()),
                    y1,
                ),
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
    pub(crate) fn select_notes_in_box(&mut self, from_tick: u64, to_tick: u64, from_pitch: u8, to_pitch: u8) {
        let (low_tick, high_tick) = (from_tick.min(to_tick), from_tick.max(to_tick));
        let (low_pitch, high_pitch) = (from_pitch.min(to_pitch), from_pitch.max(to_pitch));
        let instrument = self.selected_instrument;
        let hits: Vec<Selection> = self.project.patterns[self.selected_pattern]
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
                pattern: self.selected_pattern,
                instrument: note.instrument_id,
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
        let Some(pattern) = self.project.patterns.get(self.selected_pattern) else {
            return;
        };
        let hits: Vec<Selection> = pattern
            .notes
            .iter()
            .filter(|note| note.start_step >= start && note.start_step < end)
            .map(|note| Selection::Note {
                pattern: self.selected_pattern,
                instrument: note.instrument_id,
                pitch: note.pitch,
                start_step: note.start_step,
            })
            .collect();
        self.selection = hits.first().copied();
        self.extra_selection = hits.iter().skip(1).copied().collect();
        self.selection_anchor = hits.first().copied();
        self.status = format!("Selected {} note(s) in the ruler range.", hits.len());
    }

    /// Velocity editor strip below the piano grid: drag over a note's column to
    /// change how hard that note is played.
    pub(crate) fn velocity_lane(
        &mut self,
        ui: &mut egui::Ui,
        context: &egui::Context,
        rect: egui::Rect,
        response: egui::Response,
        step_width: f32,
        absolute_start: u64,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(26));
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(45)),
        );
        let repeats_mode = self.control_lane == ControlLane::Repeats;
        painter.text(
            egui::pos2(rect.left() - 52.0, rect.top() + 10.0),
            egui::Align2::LEFT_CENTER,
            if repeats_mode { "Repeats" } else { "Velocity" },
            egui::FontId::proportional(9.0),
            egui::Color32::from_gray(150),
        );
        let instrument = self.selected_instrument;
        let visible_end = absolute_start.saturating_add(VISIBLE_PIANO_STEPS);
        let notes: Vec<(usize, f32, u8)> = self.project.patterns[self.selected_pattern]
            .notes
            .iter()
            .enumerate()
            .filter(|(_, note)| {
                note.instrument_id == instrument
                    && note.start_step < visible_end
                    && note.start_step.saturating_add(note.duration_steps) > absolute_start
            })
            .map(|(index, note)| {
                let value = if repeats_mode {
                    // Whole repeats map onto the lane's height, so the bars snap
                    // to integer counts instead of a 0..127 velocity scale.
                    ((u32::from(note.repeat_count) * 127) / 32).min(127) as u8
                } else {
                    note.velocity.clamp(1, 127)
                };
                (index, note.start_tick() as f32 / 16.0, value)
            })
            .collect();
        for (_, start, velocity) in &notes {
            let x = rect.left() + (start - absolute_start as f32) * step_width;
            let height = (f32::from(*velocity) / 127.0).clamp(0.0, 1.0) * (rect.height() - 6.0);
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x + 1.0, rect.bottom() - height),
                    egui::pos2((x + step_width - 1.0).max(x + 3.0), rect.bottom()),
                ),
                1.0,
                lane_color(instrument),
            );
        }
        let (pressed, down, released) = context.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_down(egui::PointerButton::Primary),
                input.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        let dragged = response.drag_stopped();
        if pressed && response.hovered() {
            self.push_history();
            self.piano_gesture = None;
        }
        if (down && (response.hovered() || response.dragged()))
            && let Some(pointer) = context.input(|input| input.pointer.interact_pos())
            && rect.expand2(egui::vec2(0.0, 40.0)).contains(pointer)
        {
            let position =
                ((pointer.x - rect.left()) / step_width + absolute_start as f32).max(0.0);
            let velocity =
                (((rect.bottom() - pointer.y) / rect.height()) * 127.0).clamp(1.0, 127.0) as u8;
            let target = notes
                .iter()
                .min_by(|left, right| {
                    (left.1 - position)
                        .abs()
                        .total_cmp(&(right.1 - position).abs())
                })
                .map(|(index, start, _)| (*index, *start));
            if let Some((index, start)) = target
                && (start - position).abs() <= 1.0
            {
                let note = &mut self.project.patterns[self.selected_pattern].notes[index];
                if repeats_mode {
                    note.repeat_count =
                        ((f32::from(velocity) / 127.0 * 32.0).round() as u8).min(32);
                } else {
                    note.velocity = velocity;
                }
                self.sync_project();
            }
        }
        if released || dragged {
            self.piano_gesture = None;
        }
    }

    pub(crate) fn handle_piano_input(
        &mut self,
        grid_rect: egui::Rect,
        response: egui::Response,
        context: &egui::Context,
        step_width: f32,
        row_height: f32,
    ) {
        let (
            pointer,
            snap,
            alt,
            secondary_down,
            secondary_pressed,
            middle_down,
            primary_pressed,
            primary_down,
            primary_released,
        ) = context.input(|input| {
            (
                input.pointer.interact_pos(),
                self.project.snap_ticks.max(1),
                input.modifiers.alt,
                input.pointer.button_down(egui::PointerButton::Secondary),
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.pointer.button_down(egui::PointerButton::Middle),
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_down(egui::PointerButton::Primary),
                input.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        let Some(pointer) = pointer else {
            if primary_released || response.drag_stopped() {
                self.finish_piano_gesture();
            }
            return;
        };
        // Right-click starts delete mode; dropping the selection first keeps the
        // following actions local to what is under the pointer.
        if secondary_pressed {
            self.clear_selection();
        }
        let inside_grid = grid_rect.contains(pointer)
            && (response.contains_pointer() || self.piano_gesture.is_some());
        if !inside_grid && !(primary_down && self.piano_gesture.is_some()) {
            if primary_released || response.drag_stopped() {
                self.finish_piano_gesture();
            }
            return;
        }
        let pointer = egui::pos2(
            pointer.x.clamp(grid_rect.left(), grid_rect.right() - 0.01),
            pointer.y.clamp(grid_rect.top(), grid_rect.bottom() - 0.01),
        );
        let raw_tick = self.piano_pan_steps.saturating_mul(16)
            + (((pointer.x - grid_rect.left()) / step_width) * 16.0)
                .floor()
                .max(0.0) as u64;
        let tick = piano_snap_tick(raw_tick, snap, alt);
        let row = ((pointer.y - grid_rect.top()) / row_height)
            .floor()
            .max(0.0) as usize;
        if row >= PIANO_ROWS {
            return;
        }
        let raw_pitch = PIANO_MIN_PITCH + (PIANO_ROWS - 1 - row) as u8;
        // Note snapping pulls drawn and moved notes onto the selected scale.
        let pitch = self.snap_pitch_to_scale(raw_pitch);
        if middle_down {
            if !inside_grid {
                return;
            }
            let delta = context.input(|input| input.pointer.delta().x);
            self.piano_pan_steps = self
                .piano_pan_steps
                .saturating_add_signed((-delta / step_width).round() as i64);
            return;
        }
        if secondary_down {
            self.push_history();
            self.remove_note_at(tick, raw_pitch);
            return;
        }
        // Ctrl+drag rubber-band selection: the Select tool has no gesture of its
        // own, so the band is tracked separately from `piano_gesture`.
        if self.piano_box_origin.is_some() {
            if primary_down {
                if let Some((from_tick, from_pitch)) = self.piano_box_origin {
                    self.select_notes_in_box(from_tick, tick, from_pitch, pitch);
                }
            } else {
                self.piano_box_origin = None;
            }
        }

        if primary_pressed {
            // `pitch` is snapped to the scale for notes we create; hit tests must
            // use the row the pointer is actually on, or notes outside the scale
            // can never be selected, moved or deleted.
            self.start_piano_gesture(tick, pitch, raw_pitch, pointer, grid_rect, step_width);
        }
        if primary_down {
            self.update_piano_gesture(tick, pitch, pointer);
        }
        if primary_released || response.drag_stopped_by(egui::PointerButton::Primary) {
            self.finish_piano_gesture();
        }
    }

    pub(crate) fn paint_tool_cursor(
        &self,
        ui: &mut egui::Ui,
        context: &egui::Context,
        grid_rect: egui::Rect,
        step_width: f32,
        row_height: f32,
    ) {
        let Some(pointer) = context
            .input(|input| input.pointer.interact_pos())
            .filter(|position| grid_rect.contains(*position))
        else {
            return;
        };
        let (middle_down, secondary_down) = context.input(|input| {
            (
                input.pointer.button_down(egui::PointerButton::Middle),
                input.pointer.button_down(egui::PointerButton::Secondary),
            )
        });
        let mut cursor_kind = match self.piano_tool {
            PianoTool::Pen => "pen",
            PianoTool::Select => "move",
            PianoTool::Brush => "brush",
            PianoTool::Join => "join",
            PianoTool::Cut => "cut",
        };
        if middle_down {
            cursor_kind = "pan";
        } else {
            let row = ((pointer.y - grid_rect.top()) / row_height)
                .floor()
                .max(0.0) as usize;
            let pitch = PIANO_MIN_PITCH + (PIANO_ROWS - 1 - row.min(PIANO_ROWS - 1)) as u8;
            let hovered_note = self.project.patterns[self.selected_pattern]
                .notes
                .iter()
                .find(|note| {
                    if note.instrument_id != self.selected_instrument || note.pitch != pitch {
                        return false;
                    }
                    let start_x = grid_rect.left()
                        + (note.start_tick() as f32 / 16.0 - self.piano_pan_steps as f32)
                            * step_width;
                    let end_x = grid_rect.left()
                        + ((note.start_tick().saturating_add(note.duration_ticks())) as f32 / 16.0
                            - self.piano_pan_steps as f32)
                            * step_width;
                    pointer.x >= start_x.max(grid_rect.left())
                        && pointer.x <= end_x.min(grid_rect.right())
                });
            if let Some(note) = hovered_note {
                let note_start_x = grid_rect.left()
                    + (note.start_tick() as f32 / 16.0 - self.piano_pan_steps as f32) * step_width;
                let note_end_x = grid_rect.left()
                    + ((note.start_tick().saturating_add(note.duration_ticks())) as f32 / 16.0
                        - self.piano_pan_steps as f32)
                        * step_width;
                cursor_kind = if secondary_down {
                    "delete"
                } else if self.piano_tool == PianoTool::Pen
                    && (pointer.x >= note_end_x - 8.0
                        || (note.start_tick() >= self.piano_pan_steps.saturating_mul(16)
                            && pointer.x <= note_start_x + 8.0))
                {
                    "resize"
                } else {
                    "move"
                };
            } else if secondary_down {
                cursor_kind = "delete";
            }
        }

        let native_icon = match cursor_kind {
            "resize" => egui::CursorIcon::ResizeHorizontal,
            "move" | "join" => egui::CursorIcon::PointingHand,
            "pan" => egui::CursorIcon::Grabbing,
            "delete" => egui::CursorIcon::NoDrop,
            _ => egui::CursorIcon::Crosshair,
        };
        context.set_cursor_icon(native_icon);
        // The cut tool previews its split point on the exact tick it will use.
        if self.piano_tool == PianoTool::Cut && !middle_down {
            let alt = context.input(|input| input.modifiers.alt);
            let raw_tick = self.piano_pan_steps.saturating_mul(16)
                + (((pointer.x - grid_rect.left()) / step_width) * 16.0)
                    .floor()
                    .max(0.0) as u64;
            let tick = piano_snap_tick(raw_tick, self.project.snap_ticks, alt);
            let cut_x = grid_rect.left()
                + (tick as f32 / 16.0 - self.piano_pan_steps as f32) * step_width;
            if cut_x >= grid_rect.left() && cut_x <= grid_rect.right() {
                let stroke = egui::Stroke::new(1.4_f32, egui::Color32::from_rgb(232, 96, 88));
                let painter = ui.painter();
                let mut y = grid_rect.top();
                while y < grid_rect.bottom() {
                    painter.line_segment(
                        [egui::pos2(cut_x, y), egui::pos2(cut_x, (y + 5.0).min(grid_rect.bottom()))],
                        stroke,
                    );
                    y += 10.0;
                }
            }
        }
        paint_svg_cursor(ui, cursor_kind, pointer);
    }

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

