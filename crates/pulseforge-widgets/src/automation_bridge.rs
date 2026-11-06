//! The widget side of automation: targets, live values and menus.

use eframe::egui;
use pulseforge_core::project::ParameterTarget;

/// Key of the automated target list published every frame.
pub const AUTOMATED_TARGETS_KEY: &str = "pulseforge.automated_targets";

/// Key of the automated live value list published every frame.
pub const AUTOMATED_VALUES_KEY: &str = "pulseforge.automated_values";

/// True when a lane already drives `target`.
///
/// # Parameters
///
/// - `ui`: any layout, used for its context data.
/// - `target`: the parameter to check.
///
/// # Returns
///
/// `true` when a lane in the range covers the playhead.
pub fn target_is_automated(ui: &egui::Ui, target: ParameterTarget) -> bool {
    ui.data(|data| {
        data.get_temp::<Vec<ParameterTarget>>(egui::Id::new(AUTOMATED_TARGETS_KEY))
            .is_some_and(|targets| targets.contains(&target))
    })
}

/// The live engine value for `target` while a lane drives it.
pub fn target_live_value(ui: &egui::Ui, target: ParameterTarget) -> Option<f32> {
    ui.data(|data| {
        data.get_temp::<Vec<(ParameterTarget, f32)>>(egui::Id::new(AUTOMATED_VALUES_KEY))
            .and_then(|values| {
                values
                    .iter()
                    .find(|(candidate, _)| *candidate == target)
                    .map(|(_, value)| *value)
            })
    })
}

/// Asks for an automation lane on `target`; the request is picked up by the next
/// frame's update pass, which creates the lane.
pub fn request_automation_lane(context: &egui::Context, target: ParameterTarget) {
    context.data_mut(|data| {
        data.insert_temp(egui::Id::new("knob_add_automation"), target)
    });
}

/// Adds the shared "Add automation lane" entry to a control's right-click menu,
/// so sliders and drag values create lanes exactly the way a knob does.
pub fn automation_context_menu(response: &egui::Response, label: &str, target: ParameterTarget) {
    response.context_menu(|ui| {
        ui.label(label);
        if ui.button("Add automation lane...").clicked() {
            request_automation_lane(ui.ctx(), target);
            ui.close();
        }
    });
}

/// Automation affordance for controls that cannot carry a context menu
/// themselves: a `ComboBox` owns its own popup, so giving it a second menu as
/// well makes the one widget answer to two popups. The enum keeps its dropdown
/// and the lane is requested from this small button instead.
pub fn automation_lane_button(ui: &mut egui::Ui, label: &str, target: ParameterTarget) -> bool {
    let response = ui
        .small_button("A")
        .on_hover_text(format!("Automate {label}"));
    if response.clicked() {
        request_automation_lane(ui.ctx(), target);
        return true;
    }
    automation_context_menu(&response, label, target);
    false
}

