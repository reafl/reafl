//! The vertical dB fader.

use eframe::egui;

use crate::automation_bridge::{automation_context_menu, target_is_automated, target_live_value};
use crate::fader_math::{fader_gain, fader_label, fader_position};
use crate::tooltip::tooltip;

use pulseforge_core::project::ParameterTarget;

pub fn fader(
    ui: &mut egui::Ui,
    gain: &mut f32,
    width: f32,
    height: f32,
    target: Option<ParameterTarget>,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(22));
    let rail = egui::Rect::from_center_size(
        rect.center(),
        egui::vec2(6.0, (rect.height() - 10.0).max(10.0)),
    );
    painter.rect_filled(rail, 3.0, egui::Color32::from_gray(52));
    let mut changed = false;
    let live = target.and_then(|target| target_live_value(ui, target));
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let position = ((rail.bottom() - pointer.y) / rail.height()).clamp(0.0, 1.0);
        *gain = fader_gain(position);
        changed = true;
    }
    if let Some(target) = target
        && target_is_automated(ui, target)
    {
        painter.rect_stroke(
            rail.expand(2.0),
            4.0,
            egui::Stroke::new(1.6_f32, egui::Color32::from_rgb(202, 158, 238)),
            egui::StrokeKind::Outside,
        );
    }
    let position = fader_position(live.unwrap_or(*gain));
    let level_y = rail.bottom() - position * rail.height();
    let accent = egui::Color32::from_rgb(112, 208, 190);
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(rail.left(), level_y), rail.right_bottom()),
        3.0,
        accent.gamma_multiply(0.7),
    );
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(rect.center().x, level_y),
            egui::vec2((width - 4.0).max(8.0), 9.0),
        ),
        2.0,
        egui::Color32::from_gray(205),
    );
    tooltip(ui, &response, "Volume · dB scale, bottom = silence");
    if let Some(target) = target {
        automation_context_menu(
            &response,
            &format!("Volume · {}", fader_label(*gain)),
            target,
        );
    }
    changed
}

