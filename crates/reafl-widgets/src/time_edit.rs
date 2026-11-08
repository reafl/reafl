//! The time-value editor with its unit chooser.

use eframe::egui;
use reafl_core::project;

use crate::automation_bridge::automation_context_menu;

use reafl_core::project::ParameterTarget;

/// Edits a time value: the amount plus its unit.
///
/// # Parameters
///
/// - `ui`: the parent layout.
/// - `track_id`: lane id, used to keep the unit combo ids unique.
/// - `label`: caption shown before the amount.
/// - `value`: the time value to edit.
///
/// # Returns
///
/// `true` when the amount or the unit changed.
pub fn edit_time_value(
    ui: &mut egui::Ui,
    track_id: usize,
    label: &str,
    value: &mut project::TimeValue,
) -> bool {
    edit_time_value_automated(ui, track_id, label, value, None)
}

/// Time value editor whose amount can also be automated.
pub fn edit_time_value_automated(
    ui: &mut egui::Ui,
    track_id: usize,
    label: &str,
    value: &mut project::TimeValue,
    target: Option<ParameterTarget>,
) -> bool {
    let mut changed = false;
    // Value and unit share one row so the control does not wrap onto two lines.
    ui.horizontal(|ui| {
        ui.label(label);
        let amount = ui.add(
            egui::DragValue::new(&mut value.amount)
                .range(0.0..=60_000.0)
                .speed(1.0),
        );
        if let Some(target) = target {
            automation_context_menu(&amount, &format!("{label} · {:.2}", value.amount), target);
        }
        changed |= amount.changed();
        egui::ComboBox::from_id_salt(("time_unit", track_id, label))
            .selected_text(match value.unit {
                project::TimeUnit::Steps => "steps",
                project::TimeUnit::Milliseconds => "ms",
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut value.unit, project::TimeUnit::Steps, "steps")
                    .changed();
                changed |= ui
                    .selectable_value(&mut value.unit, project::TimeUnit::Milliseconds, "ms")
                    .changed();
            });
    });
    changed
}
