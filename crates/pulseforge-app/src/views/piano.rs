//! The piano roll, its gestures and note editing.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn render_piano_roll(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        profile_scope!("render_piano_roll");
        egui::Frame::NONE.inner_margin(4.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Pattern");
                let name = &self.project.patterns[self.selected_clip].name;
                egui::ComboBox::from_id_salt("piano_pattern")
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for (id, pattern) in self.project.patterns.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_clip, id, &pattern.name);
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
                ui.label("Right-drag deletes Â· Middle-drag pans Â· Alt disables snap");
            });
            ui.separator();
        });
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(ui, |ui| self.piano_grid(ui, context));
    }
}
