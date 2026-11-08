//! The floating windows of hosted and built-in plug-ins.

use crate::*;

impl ReaflApp {
    pub(crate) fn render_plugin_windows(&mut self, context: &egui::Context) {
        for instrument_id in 0..self.project.tracks.len() {
            let mut open = self.open_instrument_plugins.contains(&instrument_id);
            egui::Window::new(self.instrument_window_title(instrument_id))
                .id(egui::Id::new(("instrument_plugin_window", instrument_id)))
                .open(&mut open)
                .default_size([480.0, 360.0])
                .min_size([360.0, 240.0])
                .resizable(true)
                .show(context, |ui| {
                    self.render_instrument_plugin(instrument_id, ui)
                });
            if open {
                self.open_instrument_plugins.insert(instrument_id);
            } else {
                self.open_instrument_plugins.remove(&instrument_id);
                let host_id = reafl_core::hosted::instrument_host_id(instrument_id);
                if self.attached_editor == Some(host_id) {
                    self.detach_editor();
                }
            }
        }

        let open_effects: Vec<_> = self.open_effect_plugins.iter().copied().collect();
        for (channel, slot) in open_effects {
            let mut open = true;
            let channel_name = if channel == self.project.tracks.len() {
                "MASTER".to_owned()
            } else {
                self.project
                    .tracks
                    .get(channel)
                    .map_or_else(|| "Mixer".to_owned(), |lane| lane.name.clone())
            };
            let effect_name = self
                .effect_slot_name(channel, slot)
                .unwrap_or_else(|| "Empty insert".to_owned());
            egui::Window::new(format!(
                "{channel_name} Â· Insert {} Â· {effect_name}",
                slot + 1
            ))
            .id(egui::Id::new(("effect_plugin_window", channel, slot)))
            .open(&mut open)
            .default_size([420.0, 320.0])
            .min_size([320.0, 220.0])
            .resizable(true)
            .show(context, |ui| self.render_effect_plugin(channel, slot, ui));
            if !open
                || channel > self.project.tracks.len()
                || self.effect_slot_name(channel, slot).is_none()
            {
                self.open_effect_plugins.remove(&(channel, slot));
                let host_id = reafl_core::hosted::effect_host_id(channel, slot);
                if self.attached_editor == Some(host_id) {
                    self.detach_editor();
                }
            }
        }

        if let Some((lane, clip)) = self.sample_clip_window {
            let mut open = true;
            egui::Window::new("Sample clip")
                .id(egui::Id::new("sample_clip_window"))
                .open(&mut open)
                .default_size([560.0, 420.0])
                .min_size([420.0, 300.0])
                .resizable(true)
                .show(context, |ui| self.render_sample_clip_window(ui, lane, clip));
            if !open {
                self.sample_clip_window = None;
            }
        }

        if let Some(target) = self.plugin_selector_target {
            let mut open = true;
            egui::Window::new("Plug-in Selector")
                .id(egui::Id::new("plugin_selector_window"))
                .open(&mut open)
                .default_size([300.0, 380.0])
                .min_size([240.0, 240.0])
                .resizable(true)
                .show(context, |ui| self.render_plugin_selector(ui, target));
            if !open {
                self.plugin_selector_target = None;
            }
        }
    }
}

impl ReaflApp {}
