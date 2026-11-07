//! Keyboard and pointer input for the piano roll, and the tool cursor.

use crate::*;

impl PulseForgeApp {
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
            let hovered_note = self
                .active_notes()
                .iter()
                .find(|note| {
                    if note.pitch != pitch {
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
            let cut_x =
                grid_rect.left() + (tick as f32 / 16.0 - self.piano_pan_steps as f32) * step_width;
            if cut_x >= grid_rect.left() && cut_x <= grid_rect.right() {
                let stroke = egui::Stroke::new(1.4_f32, egui::Color32::from_rgb(232, 96, 88));
                let painter = ui.painter();
                let mut y = grid_rect.top();
                while y < grid_rect.bottom() {
                    painter.line_segment(
                        [
                            egui::pos2(cut_x, y),
                            egui::pos2(cut_x, (y + 5.0).min(grid_rect.bottom())),
                        ],
                        stroke,
                    );
                    y += 10.0;
                }
            }
        }
        paint_svg_cursor(ui, cursor_kind, pointer);
    }
}
