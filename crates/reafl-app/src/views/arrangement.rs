//! The arrangement: lanes, clips, automation rows and the timeline.

use crate::*;

impl ReaflApp {
    pub(crate) fn render_arrangement(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        profile_scope!("render_arrangement");
        // The arrangement keeps its own tool so piano-roll choices cannot place
        // clips, and Join (note-only) never becomes the arrangement tool.
        if self.arrangement_tool == PianoTool::Join {
            self.arrangement_tool = PianoTool::Pen;
        }
        ui.horizontal(|ui| {
            ui.label("Arrangement");
            ui.separator();
            tool_palette(ui, &mut self.arrangement_tool, false);
            ui.separator();
            ui.label(format!(
                "{}/{} · {} steps/bar",
                self.project.time_signature.numerator,
                self.project.time_signature.denominator,
                self.project.time_signature.steps_per_bar()
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    "Pen places · Brush repeats · Cut slices · Alt disables snap · Middle-drag pans",
                );
            });
        });
        ui.separator();
        self.render_timeline(ui, context);
    }
}
