//! The segmented level meter.

use eframe::egui;

use crate::theme::active_theme;

/// Draws a segmented level meter.
///
/// # Parameters
///
/// - `ui`: the parent layout.
/// - `level`: peak level in `0..=1`.
/// - `width`: meter width in points.
/// - `height`: meter height in points.
pub fn render_meter_sized(ui: &mut egui::Ui, level: f32, width: f32, height: f32) {
    let theme = active_theme(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme.color(theme.panel).gamma_multiply(0.5));
    let segments = (height / 5.0).floor().max(8.0) as usize;
    let active_segments = (level.clamp(0.0, 1.0) * segments as f32).ceil() as usize;
    for segment in 0..active_segments {
        let fraction = segment as f32 / segments as f32;
        let color = if fraction > 0.88 {
            theme.color(theme.meter_high)
        } else if fraction > 0.72 {
            theme.color(theme.meter_mid)
        } else {
            theme.color(theme.meter_low)
        };
        let y = rect.bottom() - (segment + 1) as f32 * (rect.height() / segments as f32);
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rect.left() + 2.0, y + 1.0),
                egui::vec2(rect.width() - 4.0, rect.height() / segments as f32 - 2.0),
            ),
            1.0,
            color,
        );
    }
}
