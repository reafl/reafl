//! The three-band EQ response curve.

use eframe::egui;

use crate::shape::eq_response_curve;

/// Draws the small three-band response display used by the inspector and the
/// effect windows.
///
/// # Parameters
///
/// - `ui`: the parent layout; the widget allocates its own space.
/// - `eq`: the shelf gains in dB.
pub fn draw_eq_curve(ui: &mut egui::Ui, eq: pulseforge_core::project::ThreeBandEq) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(192.0, 82.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(18));
    let center_y = rect.center().y;
    painter.line_segment(
        [
            egui::pos2(rect.left() + 5.0, center_y),
            egui::pos2(rect.right() - 5.0, center_y),
        ],
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(66)),
    );
    // The response maths lives in `shape`, shared with any other curve drawing.
    let curve = eq_response_curve(&eq, rect.width(), rect.height() * 0.84, 64);
    let offset = egui::vec2(rect.left(), rect.top() + rect.height() * 0.08);
    let points: Vec<egui::Pos2> = curve.points().iter().map(|point| *point + offset).collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(112, 208, 190)),
    ));
}
