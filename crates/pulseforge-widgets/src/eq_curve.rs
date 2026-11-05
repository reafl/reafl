//! The three-band EQ response curve.

use eframe::egui;
use pulseforge_core::project::ThreeBandEq;


pub fn draw_eq_curve(ui: &mut egui::Ui, eq: ThreeBandEq) {
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
    let points: Vec<_> = (0..=64)
        .map(|index| {
            let x = index as f32 / 64.0;
            let low = eq.low_db * (-((x - 0.16) / 0.24).powi(2)).exp();
            let mid = eq.mid_db * (-((x - 0.50) / 0.20).powi(2)).exp();
            let high = eq.high_db * (-((x - 0.84) / 0.24).powi(2)).exp();
            let db = (low + mid + high).clamp(-12.0, 12.0);
            egui::pos2(
                rect.left() + x * rect.width(),
                center_y - db / 12.0 * (rect.height() * 0.42),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(112, 208, 190)),
    ));
}

