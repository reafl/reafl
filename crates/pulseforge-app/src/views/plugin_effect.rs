//! Insert plug-in windows and the plug-in picker.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn render_effect_plugin(
        &mut self,
        channel: usize,
        slot_id: usize,
        ui: &mut egui::Ui,
    ) {
        let title = self
            .effect_slot_name(channel, slot_id)
            .unwrap_or_else(|| "Empty insert".to_owned());
        if self.plugin_window_header(
            ui,
            PluginTarget::Effect {
                channel,
                slot: slot_id,
            },
            &title,
        ) {
            if let Some(effect) = self.effect_slot_mut(channel, slot_id) {
                ui.checkbox(&mut effect.enabled, "Enabled");
                ui.checkbox(&mut effect.locked, "Locked");
                let mix_target = ParameterTarget::InsertEffect {
                    track_id: channel,
                    slot_id,
                    parameter: EffectParameter::Mix,
                };
                let mut mix = target_live_value(ui, mix_target).unwrap_or(effect.mix);
                let mix_response = ui.add(egui::Slider::new(&mut mix, 0.0..=1.0).text("Mix"));
                automation_context_menu(&mix_response, &format!("Mix · {mix:.2}"), mix_target);
                if mix_response.changed() {
                    effect.mix = mix;
                }
                ui.label(
                    "Routing and latency settings are not implemented for insert effects yet.",
                );
            }
            if ui.input(|input| input.pointer.any_released()) {
                self.sync_project();
            }
            return;
        }
        let track_count = self.project.tracks.len();
        let channel_label = if channel == track_count {
            "MASTER".to_owned()
        } else if let Some(lane) = self.project.tracks.get(channel) {
            lane.name.clone()
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Mixer channel no longer exists.");
            return;
        };
        let slots = if channel == track_count {
            &mut self.project.master.inserts
        } else if let Some(lane) = self.project.tracks.get_mut(channel) {
            &mut lane.inserts
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Mixer channel no longer exists.");
            return;
        };
        let Some(slot) = slots.get_mut(slot_id) else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Insert slot no longer exists.");
            return;
        };
        ui.horizontal(|ui| {
            ui.heading(format!("{} · {}", channel_label, slot.name));
            if ui.button("Select effect...").clicked() {
                self.plugin_selector_target = Some(PluginTarget::Effect {
                    channel,
                    slot: slot_id,
                });
            }
        });
        let mut changed = ui.checkbox(&mut slot.enabled, "Enabled").changed();
        changed |= render_effect_parameters(ui, slot, channel, slot_id);
        let plugin_path = slot.plugin_path.clone();
        let hosted = slot.enabled && !plugin_path.is_empty();
        if changed {
            self.sync_project();
        }
        if hosted {
            let host_id = pulseforge_core::hosted::effect_host_id(channel, slot_id);
            if self.hosted.contains_key(&host_id) {
                self.render_hosted_plugin(ui, host_id);
            } else {
                ui.horizontal(|ui| {
                    ui.label("Hosted plug-in is not loaded.");
                    if ui.button("Load plug-in").clicked() {
                        self.load_host(host_id, &plugin_path, false);
                    }
                });
                if let Some(error) = self.hosted_errors.get(&host_id) {
                    ui.colored_label(egui::Color32::from_rgb(230, 140, 110), error);
                }
            }
        }
    }

    pub(crate) fn render_plugin_selector(&mut self, ui: &mut egui::Ui, target: PluginTarget) {
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(
                egui::TextEdit::singleline(&mut self.plugin_search)
                    .desired_width(200.0)
                    .hint_text("Plug-in name"),
            );
            if ui.small_button("Clear").clicked() {
                self.plugin_search.clear();
            }
        });
        let filter = self.plugin_search.to_ascii_lowercase();
        let matches = |name: &str| filter.is_empty() || name.to_ascii_lowercase().contains(&filter);
        ui.separator();
        match target {
            PluginTarget::Instrument(instrument_id) => {
                let Some(track) = self.project.tracks.get(instrument_id) else {
                    ui.colored_label(egui::Color32::LIGHT_RED, "Track no longer exists.");
                    return;
                };
                ui.label(format!("Choose a plug-in for {}", track.name));
                let current = track
                    .generator()
                    .and_then(|generator| generator.kind.synth_kind())
                    .unwrap_or(SynthKind::Sine);
                for kind in SynthKind::BUILTIN {
                    if !matches(synth_kind_label(kind)) {
                        continue;
                    }
                    if ui
                        .selectable_label(current == kind, synth_kind_label(kind))
                        .clicked()
                    {
                        // Choosing a built-in must clear any hosted plug-in so
                        // the wrapper window cannot claim the slot.
                        let host_id = pulseforge_core::hosted::instrument_host_id(instrument_id);
                        self.unload_host(host_id);
                        let track = &mut self.project.tracks[instrument_id];
                        track.enabled = true;
                        if let Some(generator) = track.generator_mut() {
                            generator.kind = GeneratorKind::Synth(kind);
                            generator.plugin_path.clear();
                        }
                        self.open_instrument_plugins.insert(instrument_id);
                        self.plugin_selector_target = None;
                        self.sync_project();
                        break;
                    }
                }
                ui.separator();
                ui.label(egui::RichText::new("Installed plug-ins").strong());
                for plugin in self.external_plugins.clone() {
                    if !matches(&plugin.label()) {
                        continue;
                    }
                    let active = self
                        .project
                        .tracks
                        .get(instrument_id)
                        .and_then(|track| track.generator())
                        .is_some_and(|generator| generator.plugin_path == plugin.path);
                    if ui
                        .selectable_label(active, plugin.label())
                        .on_hover_text(&plugin.path)
                        .clicked()
                    {
                        let host_id = pulseforge_core::hosted::instrument_host_id(instrument_id);
                        let path = plugin.path.clone();
                        self.load_host(host_id, &path, true);
                        let track = &mut self.project.tracks[instrument_id];
                        track.name = plugin.name.clone();
                        track.enabled = true;
                        if let Some(generator) = track.generator_mut() {
                            generator.plugin_path = plugin.path.clone();
                            // An external generator must not keep the built-in
                            // voice it had before, otherwise it renders as that
                            // synth.
                            generator.kind = GeneratorKind::Synth(SynthKind::External);
                        }
                        self.open_instrument_plugins.insert(instrument_id);
                        self.plugin_selector_target = None;
                        self.sync_project();
                        break;
                    }
                }
            }
            PluginTarget::Effect { channel, slot } => {
                ui.label("Choose an insert effect");
                let options = [
                    ("3-Band EQ", "3-Band EQ"),
                    ("Reverb", "Reverb"),
                    ("Plate Reverb", "Plate Reverb"),
                    ("Delay", "Delay"),
                    ("Tape Echo", "Tape Echo"),
                    ("Juno Chorus", "Juno Chorus"),
                    ("Phaser", "Phaser"),
                    ("Waveshaper", "Waveshaper"),
                ];
                let current = self.effect_slot_name(channel, slot);
                for (name, label) in options {
                    if !matches(label) {
                        continue;
                    }
                    if ui
                        .selectable_label(current.as_deref() == Some(name), label)
                        .clicked()
                    {
                        if let Some(effect) = self.effect_slot_mut(channel, slot) {
                            effect.name = name.to_owned();
                            effect.plugin_path.clear();
                            effect.enabled = true;
                        }
                        let host_id = pulseforge_core::hosted::effect_host_id(channel, slot);
                        self.unload_host(host_id);
                        self.open_effect_plugins.insert((channel, slot));
                        self.plugin_selector_target = None;
                        self.sync_project();
                        break;
                    }
                }
                if ui.button("Clear insert").clicked() {
                    if let Some(effect) = self.effect_slot_mut(channel, slot) {
                        effect.name.clear();
                        effect.plugin_path.clear();
                        effect.enabled = false;
                    }
                    let host_id = pulseforge_core::hosted::effect_host_id(channel, slot);
                    self.unload_host(host_id);
                    self.plugin_selector_target = None;
                    self.sync_project();
                }
                ui.separator();
                ui.label(egui::RichText::new("Installed plug-ins").strong());
                if self.external_plugins.is_empty() {
                    ui.label("None discovered. Configure folders in Settings ? Plug-ins.");
                }
                for plugin in self.external_plugins.clone() {
                    if !matches(&plugin.label()) {
                        continue;
                    }
                    let active = self
                        .effect_slot_name(channel, slot)
                        .is_some_and(|name| name == plugin.label());
                    if ui
                        .selectable_label(active, plugin.label())
                        .on_hover_text(&plugin.path)
                        .clicked()
                    {
                        if let Some(effect) = self.effect_slot_mut(channel, slot) {
                            effect.name = plugin.label();
                            effect.plugin_path = plugin.path.clone();
                            effect.enabled = true;
                        }
                        let host_id = pulseforge_core::hosted::effect_host_id(channel, slot);
                        let path = plugin.path.clone();
                        self.load_host(host_id, &path, false);
                        self.open_effect_plugins.insert((channel, slot));
                        self.plugin_selector_target = None;
                        self.sync_project();
                        break;
                    }
                }
                if ui.button("Plug-in settings").clicked() {
                    self.windows.settings = true;
                    self.settings_tab = SettingsTab::Plugins;
                }
            }
        }
    }
}
