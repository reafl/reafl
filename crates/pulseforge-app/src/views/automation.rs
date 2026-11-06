//! The automation window.
//!
//! The window is an assembly of three helpers that live in their own modules:
//! [`automation_target`](super::automation_target) owns the target picker,
//! [`automation_formula`](super::automation_formula) the formula tool and
//! [`automation_tracks`](super::automation_tracks) the editable list of lanes.

use crate::*;

impl PulseForgeApp {
    /// Draws the automation window.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI.
    pub(crate) fn render_automation_window(&mut self, ui: &mut egui::Ui) {
        self.clamp_automation_indices();
        ui.label("Automate every continuous built-in instrument and insert-effect control.");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.automation_effect_mode, false, "Instrument");
            ui.selectable_value(&mut self.automation_effect_mode, true, "Insert FX");
        });
        if let Some(target) = self.automation_target_selector(ui) {
            self.automation_add_lane_button(ui, target);
        }
        ui.separator();
        self.automation_formula_tool(ui);
        ui.separator();
        self.automation_track_list(ui);
    }
}
