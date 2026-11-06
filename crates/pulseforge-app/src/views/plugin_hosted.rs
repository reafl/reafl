//! Wrappers for hosted VST3/CLAP editors inside egui.

use crate::*;

impl PulseForgeApp {
    /// Hosted plug-in window body: identity, parameter knobs and the editor area.
    pub(crate) fn render_hosted_plugin(&mut self, ui: &mut egui::Ui, host_id: u64) {
        let Some(meta) = self.hosted_meta.get(&host_id) else {
            let message = self
                .hosted_errors
                .get(&host_id)
                .cloned()
                .unwrap_or_else(|| "Plug-in is not loaded.".to_owned());
            ui.colored_label(egui::Color32::from_rgb(220, 120, 110), message);
            return;
        };
        // Only the header lives in egui; the plug-in draws its own parameters in
        // its own editor, so no parameter strip is shown here.
        let info = meta.info.clone();
        let preferred = meta.editor_size;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&info.name).strong());
            if !info.vendor.is_empty() {
                ui.label(
                    egui::RichText::new(&info.vendor)
                        .small()
                        .color(egui::Color32::from_gray(150)),
                );
            }
            if let Some((id, name)) = self.last_tweaked.clone()
                && id == host_id
            {
                if ui
                    .button("Automate")
                    .on_hover_text(format!("Create an automation clip for {name}"))
                    .clicked()
                {
                    self.create_plugin_automation(host_id, &name);
                }
                ui.label(
                    egui::RichText::new(name)
                        .small()
                        .color(egui::Color32::from_gray(160)),
                );
            }
        });
        if let Some(error) = self.hosted_errors.get(&host_id).cloned() {
            ui.colored_label(egui::Color32::from_rgb(230, 140, 110), error);
        }
        // The editor fills whatever space the window gives it, so resizing the
        // window resizes the plug-in.
        let scale = self.last_pixels_per_point.max(0.5);
        let min = egui::vec2(
            (preferred.0 as f32 / scale).max(320.0),
            (preferred.1 as f32 / scale).max(200.0),
        );
        let available = ui.available_size();
        let size = egui::vec2(available.x.max(min.x), available.y.max(min.y));
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 4.0, egui::Color32::from_gray(10));
        self.sync_editor_window(host_id, rect);
        self.resize_editor_to(host_id, rect);
    }

    /// External plug-in wrapper: shows the bundle identity plus an editor area.
    /// Fallback shown until an instrument's bundle has been loaded.
    pub(crate) fn render_external_plugin_wrapper(
        &mut self,
        ui: &mut egui::Ui,
        instrument_id: usize,
    ) {
        let host_id = pulseforge_core::hosted::instrument_host_id(instrument_id);
        if self.hosted.contains_key(&host_id) {
            self.render_hosted_plugin(ui, host_id);
            return;
        }
        let path = self
            .project
            .instruments
            .get(instrument_id)
            .map(|instrument| instrument.plugin_path.clone())
            .unwrap_or_default();
        let format = self
            .external_plugins
            .iter()
            .find(|plugin| plugin.path == path)
            .map_or("Plug-in", |plugin| plugin.format.label());
        let metadata = self.metadata_for(&path);
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_gray(18))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format).strong().color(self.ui_accent()));
                    ui.label(egui::RichText::new(&metadata.name).strong());
                });
                ui.label(egui::RichText::new(&path).small());
                if let Some(error) = self.hosted_errors.get(&host_id) {
                    ui.colored_label(egui::Color32::from_rgb(230, 140, 110), error);
                }
                if ui.button("Load plug-in").clicked() {
                    self.load_host(host_id, &path, true);
                }
            });
    }
}
