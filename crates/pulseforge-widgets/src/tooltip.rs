//! Small hover and text helpers.

use eframe::egui;
use pulseforge_settings::settings;

/// Key of the stored tooltip mode.
pub const TOOLTIP_MODE_KEY: &str = "pulseforge.tooltip_mode";

/// Key of the stored tooltip delay, in seconds.
pub const TOOLTIP_DELAY_KEY: &str = "pulseforge.tooltip_delay";

/// Key the hovered widget text is published under for the status bar.
pub const HOVER_TEXT_KEY: &str = "pulseforge.hover_text";

/// Applies the configured tooltip policy to a widget.
///
/// # Parameters
///
/// - `ui`: the layout the widget lives in.
/// - `response`: the widget to describe.
/// - `text`: what to show when the pointer rests on it.
pub fn tooltip(ui: &egui::Ui, response: &egui::Response, text: &str) {
    let mode =
        ui.data(|data| data.get_temp::<settings::TooltipMode>(egui::Id::new(TOOLTIP_MODE_KEY)));
    let delay = ui
        .data(|data| data.get_temp::<f32>(egui::Id::new(TOOLTIP_DELAY_KEY)))
        .unwrap_or(0.6);
    if response.hovered() {
        ui.data_mut(|data| {
            data.insert_temp(egui::Id::new(HOVER_TEXT_KEY), text.to_owned());
        });
        let id = response.id.with("tooltip_timer");
        let now = ui.input(|input| input.time);
        if ui.data(|data| data.get_temp::<f64>(id)).is_none() {
            ui.data_mut(|data| data.insert_temp(id, now));
        }
    } else {
        ui.data_mut(|data| {
            data.remove::<f64>(response.id.with("tooltip_timer"));
        });
    }
    match mode.unwrap_or_default() {
        settings::TooltipMode::StatusBar => {}
        settings::TooltipMode::Hover => {
            // egui has no delayed hover text, so the delay is tracked per widget.
            if response.hovered() {
                let id = response.id.with("tooltip_timer");
                let elapsed = ui
                    .data(|data| data.get_temp::<f64>(id))
                    .map_or(0.0, |started| ui.input(|input| input.time) - started);
                if elapsed >= f64::from(delay) {
                    response.clone().on_hover_text(text);
                }
            }
        }
        settings::TooltipMode::Immediate => {
            response.clone().on_hover_text(text);
        }
    }
}
