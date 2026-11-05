//! Colour tokens, theme application and colour maths.

use super::*;

pub(crate) fn apply_theme(context: &egui::Context, theme: &settings::Theme) {
    let panel = theme.color(theme.panel);
    let background = theme.color(theme.background);
    let accent = theme.color(theme.accent);
    let text = theme.color(theme.text);
    let mut visuals = if theme.background.iter().all(|channel| *channel > 128) {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.override_text_color = Some(text);
    visuals.window_fill = panel;
    visuals.panel_fill = background;
    visuals.extreme_bg_color = panel.gamma_multiply(0.55);
    visuals.faint_bg_color = panel.gamma_multiply(1.25);
    visuals.selection.bg_fill = accent.gamma_multiply(0.45);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, accent);
    visuals.widgets.active.bg_fill = accent.gamma_multiply(0.5);
    visuals.widgets.hovered.bg_fill = panel.gamma_multiply(1.35);
    visuals.widgets.inactive.bg_fill = panel.gamma_multiply(1.1);
    visuals.widgets.noninteractive.bg_fill = panel;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, panel.gamma_multiply(1.6));
    context.set_visuals(visuals);
    let scale = theme.font_scale.clamp(0.75, 1.6);
    context.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(7.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.visuals.window_corner_radius = egui::CornerRadius::same(8);
        for font in style.text_styles.values_mut() {
            font.size = (font.size * scale).max(6.0);
        }
    });
    context.data_mut(|data| {
        data.insert_temp(egui::Id::new(THEME_KEY), theme.clone());
    });
}

/// Context-data key holding the active theme.
pub(crate) const THEME_KEY: &str = "active_theme";

/// Active theme for free drawing helpers.
pub(crate) fn active_theme(context: &egui::Context) -> settings::Theme {
    context
        .data(|data| data.get_temp::<settings::Theme>(egui::Id::new(THEME_KEY)))
        .unwrap_or_default()
}

pub(crate) fn lane_color(index: usize) -> egui::Color32 {
    const COLORS: [egui::Color32; 8] = [
        egui::Color32::from_rgb(211, 115, 91),
        egui::Color32::from_rgb(209, 163, 77),
        egui::Color32::from_rgb(92, 178, 133),
        egui::Color32::from_rgb(91, 157, 208),
        egui::Color32::from_rgb(154, 119, 205),
        egui::Color32::from_rgb(198, 112, 167),
        egui::Color32::from_rgb(88, 184, 181),
        egui::Color32::from_rgb(176, 175, 91),
    ];
    COLORS[index % COLORS.len()]
}

pub(crate) fn color_lerp(start: egui::Color32, end: egui::Color32, amount: f32) -> egui::Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |left: u8, right: u8| {
        (f32::from(left) + (f32::from(right) - f32::from(left)) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    egui::Color32::from_rgb(
        channel(start.r(), end.r()),
        channel(start.g(), end.g()),
        channel(start.b(), end.b()),
    )
}

pub(crate) fn point_segment_distance(point: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let amount = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * amount)
}

