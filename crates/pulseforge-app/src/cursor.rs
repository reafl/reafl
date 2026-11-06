//! The custom SVG tool cursors.

use crate::*;

pub(crate) fn svg_cursor_points(kind: &str) -> Option<Vec<[f32; 2]>> {
    let marker = format!(r#"data-cursor="{kind}""#);
    let line = TOOL_CURSOR_SVG
        .lines()
        .find(|line| line.contains(&marker))?;
    let encoded = line.split_once("points=\"")?.1.split_once('"')?.0;
    let points: Vec<_> = encoded
        .split_whitespace()
        .filter_map(|point| {
            let (x, y) = point.split_once(',')?;
            Some([x.parse().ok()?, y.parse().ok()?])
        })
        .collect();
    (!points.is_empty()).then_some(points)
}

pub(crate) fn paint_svg_cursor(ui: &egui::Ui, kind: &str, position: egui::Pos2) {
    let Some(points) = svg_cursor_points(kind) else {
        return;
    };
    let vertices: Vec<_> = points
        .into_iter()
        .map(|[x, y]| position + egui::vec2(x * 1.15, y * 1.15))
        .collect();
    ui.painter().add(egui::Shape::convex_polygon(
        vertices,
        egui::Color32::from_rgba_unmultiplied(92, 228, 199, 205),
        egui::Stroke::new(1.0_f32, egui::Color32::WHITE),
    ));
}
