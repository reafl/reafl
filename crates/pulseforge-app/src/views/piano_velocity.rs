//! The velocity and repeats lane under the piano roll.

use crate::*;

impl PulseForgeApp {
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

        let visible_end = absolute_start.saturating_add(VISIBLE_PIANO_STEPS);
        let notes: Vec<(usize, f32, u8)> = self
            .active_notes()
            .iter()
            .enumerate()
            .filter(|(_, note)| {
                note.start_step < visible_end
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
                lane_color(self.selected_instrument),
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
                let Some(note) = self.active_notes_mut().and_then(|notes| notes.get_mut(index)) else {
                    return;
                };
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
}
