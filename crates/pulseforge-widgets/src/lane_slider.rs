//! A slider that can become an automation target.

use eframe::egui;
use pulseforge_core::project::ParameterTarget;

use crate::automation_bridge::{automation_context_menu, target_live_value};

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

