//! A slider that can become an automation target.

use eframe::egui;
use reafl_core::project::ParameterTarget;

use crate::automation_bridge::{automation_context_menu, target_live_value};

/// Slider for one of an automation lane own parameters.
///
/// While a lane drives the value the slider sits at the engine value; a
/// drag still writes through to the stored value.
///
/// # Parameters
///
/// - `ui`: the parent layout.
/// - `label`: caption shown beside the slider.
/// - `value`: the parameter to edit.
/// - `range`: inclusive bounds.
/// - `target`: the automation target this control stands for.
///
/// # Returns
///
/// `true` when the value changed this frame.
pub fn lane_parameter_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    target: ParameterTarget,
) -> bool {
    // While a lane drives the parameter the slider sits at the engine value, so
    // the control shows what is actually heard. Dragging still writes through.
    let mut shown = target_live_value(ui, target).unwrap_or(*value);
    let response = ui.add(egui::Slider::new(&mut shown, range).text(label));
    automation_context_menu(&response, &format!("{label} · {shown:.2}"), target);
    if response.changed() {
        *value = shown;
        true
    } else {
        false
    }
}
