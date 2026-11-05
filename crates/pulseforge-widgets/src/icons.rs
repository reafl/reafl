//! Vector glyphs and the icon buttons built from them. 
use eframe::egui;

use crate::tooltip::tooltip;

/// Vector toolbar icons.
///
/// These are painted with shapes rather than font glyphs because the bundled
/// fonts do not cover symbols such as U+23FB or U+1F512, which rendered as
/// empty boxes. Shapes also stay crisp at any DPI.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Rack,
    Roll,
    Mixer,
    Routing,
    Automation,
    Samples,
    Midi,
    Settings,
    Render,
    Debug,
    Undo,
    Redo,
    Play,
    Stop,
    Loop,
    Metronome,
    Power,
    Solo,
    ArrowDown,
    ArrowRight,
}

/// Draws `icon` inside `rect` using strokes only.
pub fn paint_icon(painter: &egui::Painter, rect: egui::Rect, icon: Icon, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.4_f32, color);
    let mid = rect.center();
    let size = rect.width().min(rect.height());
    let unit = size * 0.5;
    let at = |x: f32, y: f32| egui::pos2(mid.x + x * unit, mid.y + y * unit);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    match icon {
        Icon::Rack => {
            for row in 0..3 {
                let y = -0.6 + row as f32 * 0.6;
                line((-0.7, y), (0.7, y));
                painter.circle_filled(at(-0.45 + row as f32 * 0.25, y), unit * 0.12, color);
            }
        }
        Icon::Roll => {
            for step in 0..3 {
                let x = -0.6 + step as f32 * 0.6;
                line((x, -0.7), (x, 0.7));
            }
            for step in 0..3 {
                let y = -0.6 + step as f32 * 0.6;
                line((-0.7, y), (0.7, y));
            }
        }
        Icon::Mixer => {
            for (index, height) in [0.6_f32, 0.1, 0.35].into_iter().enumerate() {
                let x = -0.5 + index as f32 * 0.5;
                line((x, -0.7), (x, 0.7));
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        at(x, -0.7 + height),
                        egui::vec2(unit * 0.5, unit * 0.24),
                    ),
                    1.0,
                    color,
                );
            }
        }
        Icon::Routing => {
            painter.circle_stroke(at(-0.55, 0.45), unit * 0.22, stroke);
            painter.circle_stroke(at(0.55, -0.45), unit * 0.22, stroke);
            line((-0.35, 0.3), (0.35, -0.3));
        }
        Icon::Automation => {
            let points = [
                at(-0.7, 0.55),
                at(-0.2, 0.05),
                at(0.25, 0.3),
                at(0.7, -0.55),
            ];
            for pair in points.windows(2) {
                painter.line_segment([pair[0], pair[1]], stroke);
            }
            for point in points {
                painter.circle_filled(point, unit * 0.1, color);
            }
        }
        Icon::Samples => {
            line((-0.7, 0.0), (0.7, 0.0));
            for index in 0..7 {
                let x = -0.6 + index as f32 * 0.2;
                let amplitude = if index % 2 == 0 { 0.5 } else { 0.22 };
                line((x, -amplitude), (x, amplitude));
            }
        }
        Icon::Midi => {
            painter.rect_stroke(
                egui::Rect::from_center_size(mid, egui::vec2(unit * 1.4, unit * 1.0)),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            for index in 0..3 {
                let x = -0.35 + index as f32 * 0.35;
                painter.circle_filled(at(x, 0.0), unit * 0.09, color);
            }
        }
        Icon::Settings => {
            painter.circle_stroke(mid, unit * 0.5, stroke);
            for index in 0..6 {
                let angle = index as f32 * std::f32::consts::TAU / 6.0;
                let (sin, cos) = angle.sin_cos();
                line((cos * 0.55, sin * 0.55), (cos * 0.85, sin * 0.85));
            }
            painter.circle_filled(mid, unit * 0.14, color);
        }
        Icon::Render => {
            line((0.0, -0.7), (0.0, 0.25));
            line((-0.35, -0.05), (0.0, 0.3));
            line((0.35, -0.05), (0.0, 0.3));
            line((-0.7, 0.6), (0.7, 0.6));
        }
        Icon::Debug => {
            // Oscilloscope inside a frame: a "profile trace" glyph.
            painter.rect_stroke(
                egui::Rect::from_center_size(mid, egui::vec2(unit * 1.5, unit * 1.2)),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            let points = [at(-0.55, 0.1), at(-0.2, -0.4), at(0.1, 0.4), at(0.5, -0.25)];
            for pair in points.windows(2) {
                painter.line_segment([pair[0], pair[1]], stroke);
            }
        }
        Icon::Undo | Icon::Redo => {
            let direction = if icon == Icon::Undo { -1.0 } else { 1.0 };
            line((0.55 * direction, -0.5), (0.0, -0.5));
            line((0.0, -0.5), (0.0, 0.45));
            line((-0.55 * direction, 0.45), (0.55 * direction, 0.45));
            line((-0.55 * direction, 0.45), (-0.55 * direction, -0.5));
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![at(-0.45, -0.6), at(-0.45, 0.6), at(0.6, 0.0)],
                color,
                egui::Stroke::NONE,
            ));
        }
        Icon::Stop => {
            painter.rect_filled(
                egui::Rect::from_center_size(mid, egui::vec2(unit * 1.1, unit * 1.1)),
                1.5,
                color,
            );
        }
        Icon::Loop => {
            painter.circle_stroke(mid, unit * 0.6, stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![at(0.35, -0.85), at(0.85, -0.55), at(0.35, -0.3)],
                color,
                egui::Stroke::NONE,
            ));
        }
        Icon::Metronome => {
            line((-0.5, 0.7), (-0.15, -0.7));
            line((0.5, 0.7), (0.15, -0.7));
            line((-0.5, 0.7), (0.5, 0.7));
            line((0.0, 0.2), (0.4, -0.6));
        }
        Icon::Power => {
            painter.circle_stroke(mid, unit * 0.72, stroke);
            line((0.0, -0.95), (0.0, -0.15));
        }
        Icon::Solo => {
            painter.circle_stroke(mid, unit * 0.55, stroke);
            painter.circle_filled(mid, unit * 0.18, color);
        }
        Icon::ArrowDown => {
            line((-0.35, -0.2), (0.0, 0.3));
            line((0.35, -0.2), (0.0, 0.3));
        }
        Icon::ArrowRight => {
            line((-0.2, -0.35), (0.3, 0.0));
            line((-0.2, 0.35), (0.3, 0.0));
        }
    }
}

/// Square icon button. Icons are painted, so no font glyph coverage is needed.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
    if response.hovered() {
        ui.painter()
            .rect_stroke(rect, 3.0, visuals.bg_stroke, egui::StrokeKind::Inside);
    }
    let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
    paint_icon(ui.painter(), inner, icon, visuals.fg_stroke.color);
    tooltip(ui, &response, hint);
    response
}

/// Icon button that also shows an on/off state.
///
/// Used by the transport switches so a lit button means "on" without having to
/// read the status line.
pub fn icon_button_toggle(
    ui: &mut egui::Ui,
    icon: Icon,
    hint: &str,
    active: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    let fill = if active {
        ui.visuals().selection.bg_fill
    } else {
        visuals.weak_bg_fill
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    if active || response.hovered() {
        ui.painter()
            .rect_stroke(rect, 3.0, visuals.bg_stroke, egui::StrokeKind::Inside);
    }
    let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
    paint_icon(
        ui.painter(),
        inner,
        icon,
        if active {
            egui::Color32::WHITE
        } else {
            visuals.fg_stroke.color
        },
    );
    tooltip(ui, &response, hint);
    response
}

/// Word used by status messages for a power switch.
pub fn power_word(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

