//! The mixer strips and the channel inspector.

use crate::*;

impl PulseForgeApp {
    /// Adds a mixer channel (and its arrangement lane) and selects it.
    pub(crate) fn add_mixer_channel(&mut self) {
        let old_master_id = self.project.lanes.len();
        self.project.add_lane();
        if self.automation_channel_id == old_master_id {
            self.automation_channel_id = old_master_id + 1;
        }
        self.selected_lane = self.project.lanes.len() - 1;
        self.selected_mixer_channel = self.selected_lane;
        self.sync_project();
    }

    pub(crate) fn render_mixer(&mut self, ui: &mut egui::Ui) {
        profile_scope!("render_mixer");
        ui.horizontal(|ui| {
            ui.label("Mixer");
            if ui
                .checkbox(&mut self.project.mixer_collapsed, "Compact strips")
                .changed()
            {
                self.sync_project();
            }
            if ui.button("+ Mixer channel").clicked() {
                self.add_mixer_channel();
            }
            ui.label("Master stays pinned at the left edge.");
        });
        ui.separator();
        let panel_width = 286.0_f32.min((ui.available_width() * 0.42).max(220.0));
        let strips_width = (ui.available_width() - panel_width - 12.0).max(180.0);
        let content_height = ui.available_height().max(260.0);
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(
                    if self.project.mixer_collapsed {
                        150.0
                    } else {
                        232.0
                    },
                    content_height,
                ),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let master_id = self.project.lanes.len();
                    let master_meter = self.meter_level(master_id);
                    let open_inserts = self.open_effect_plugins.clone();
                    let result = mixer_strip(
                        ui,
                        MixerStrip {
                            channel_id: master_id,
                            name: None,
                            pinned: None,
                            gain: &mut self.project.master.gain,
                            pan: &mut self.project.master.pan,
                            width: None,
                            eq: &mut self.project.master.eq,
                            meter: master_meter,
                            selected: self.selected_mixer_channel == master_id,
                            collapsed: self.project.mixer_collapsed,
                            is_master: true,
                            enabled: true,
                            locked: false,
                            solo: false,
                            inserts: Some(&mut self.project.master.inserts),
                            fx_bypass: None,
                            open_inserts: &open_inserts,
                        },
                    );
                    if result.selected {
                        self.select_channel(master_id);
                    }
                    // Same ordering rule as the lane strips: apply the action,
                    // then push the project so the engine sees the change.
                    for action in result.actions {
                        self.apply_context_action(action);
                    }
                    if result.changed {
                        self.sync_project();
                    }
                },
            );
            ui.separator();
            let lane_strip_width = (strips_width
                - if self.project.mixer_collapsed {
                    150.0
                } else {
                    232.0
                }
                - 8.0)
                .max(120.0);
            ui.allocate_ui_with_layout(
                egui::vec2(lane_strip_width, content_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut order: Vec<_> = (0..self.project.lanes.len()).collect();
                                order.sort_by_key(|lane| !self.project.lanes[*lane].pinned);
                                let mut any_changed = false;
                                let mut actions = Vec::new();
                                for lane_id in order {
                                    let meter = self.meter_level(lane_id);
                                    let open_inserts = self.open_effect_plugins.clone();
                                    let result = {
                                        let lane = &mut self.project.lanes[lane_id];
                                        let (enabled, locked, solo) =
                                            (lane.enabled, lane.locked, lane.solo);
                                        mixer_strip(
                                            ui,
                                            MixerStrip {
                                                channel_id: lane_id,
                                                name: Some(&mut lane.name),
                                                pinned: Some(&mut lane.pinned),
                                                gain: &mut lane.gain,
                                                pan: &mut lane.pan,
                                                width: Some(&mut lane.width),
                                                eq: &mut lane.eq,
                                                meter,
                                                selected: self.selected_mixer_channel == lane_id,
                                                collapsed: self.project.mixer_collapsed,
                                                is_master: false,
                                                enabled,
                                                locked,
                                                solo,
                                                inserts: Some(&mut lane.inserts),
                                                fx_bypass: Some(&mut lane.fx_bypass),
                                                open_inserts: &open_inserts,
                                            },
                                        )
                                    };
                                    if result.selected {
                                        self.selected_mixer_channel = lane_id;
                                        self.selected_lane = lane_id;
                                        self.selection = Some(Selection::MixerChannel(lane_id));
                                    }
                                    any_changed |= result.changed;
                                    actions.extend(result.actions);
                                }
                                // Actions first: they mutate the model (insert
                                // enable, solo, favourites), and the sync below is
                                // what actually reaches the audio engine.
                                for action in actions {
                                    self.apply_context_action(action);
                                }
                                if any_changed {
                                    self.sync_project();
                                }
                                // Add-track affordance at the end of the row, so
                                // channels can be added where they appear. Right
                                // click adds one pre-loaded with a saved chain.
                                let add_track = ui
                                    .add_sized(
                                        [86.0, 34.0],
                                        egui::Button::new("+ Track")
                                            .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text(
                                        "Left click: add a channel. Right click: add one from a preset.",
                                    );
                                let mut add_from_preset: Option<String> = None;
                                add_track.context_menu(|ui| {
                                    ui.label("Add channel from preset");
                                    if self.chain_presets.is_empty() {
                                        ui.label("No presets saved yet.");
                                    }
                                    for name in self.chain_presets.clone() {
                                        if ui.button(&name).clicked() {
                                            add_from_preset = Some(name);
                                            ui.close();
                                        }
                                    }
                                });
                                if add_track.clicked() {
                                    self.add_mixer_channel();
                                }
                                if let Some(name) = add_from_preset {
                                    self.add_mixer_channel();
                                    let channel = self.selected_mixer_channel;
                                    match load_chain_preset(&name) {
                                        Ok(preset) => {
                                            if let Some(lane) =
                                                self.project.lanes.get_mut(channel)
                                            {
                                                for (slot, loaded) in
                                                    lane.inserts.iter_mut().zip(preset)
                                                {
                                                    *slot = loaded;
                                                }
                                            }
                                            self.sync_project();
                                            // Instantiate the chain's plug-ins so
                                            // their saved state is applied.
                                            self.restore_hosted_plugins();
                                            self.status = format!(
                                                "Added a channel with preset '{name}'."
                                            );
                                        }
                                        Err(error) => self.status = error,
                                    }
                                }
                            });
                        });
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(panel_width, content_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.render_mixer_inspector(ui),
            );
        });
    }

    pub(crate) fn render_mixer_inspector(&mut self, ui: &mut egui::Ui) {
        let channel = self.selected_mixer_channel.min(self.project.lanes.len());
        let name = if channel == self.project.lanes.len() {
            "MASTER".to_owned()
        } else {
            self.project.lanes[channel].name.clone()
        };
        ui.heading(format!("Track · {name}"));
        ui.separator();
        let is_master = channel == self.project.lanes.len();
        // Level and pan live on the strips, so the inspector shows what a strip
        // cannot: EQ, stereo width and the insert chain, on one page.
        let (eq, inserts, width) = if is_master {
            (
                &mut self.project.master.eq,
                &mut self.project.master.inserts,
                None,
            )
        } else {
            let lane = &mut self.project.lanes[channel];
            (&mut lane.eq, &mut lane.inserts, Some(&mut lane.width))
        };
        let mut changed = false;
        ui.label(egui::RichText::new("3-BAND EQ").strong());
        draw_eq_curve(ui, *eq);
        ui.horizontal(|ui| {
            changed |= Knob::new("LOW", -12.0, 12.0, " dB").show(ui, &mut eq.low_db);
            changed |= Knob::new("MID", -12.0, 12.0, " dB").show(ui, &mut eq.mid_db);
            changed |= Knob::new("HIGH", -12.0, 12.0, " dB").show(ui, &mut eq.high_db);
            if let Some(width) = width {
                changed |= Knob::new("WIDTH", 0.0, 2.0, "")
                    .with_default(1.0)
                    .show(ui, width);
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("INSERTS").strong());
        // Chain presets save and restore the whole insert list as one JSON file,
        // so a channel strip can be reused on another lane or in another project.
        // Save/load are deferred past the borrow of `inserts` below, because
        // capturing hosted plug-in state and re-instantiating it both need `self`.
        let mut save_preset_request: Option<String> = None;
        let mut preset_loaded = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.chain_preset_name)
                    .desired_width(120.0)
                    .hint_text("Preset name"),
            );
            if ui.button("Save chain").clicked() {
                save_preset_request = Some(self.chain_preset_name.clone());
            }
            ui.menu_button("Load chain", |ui| {
                if self.chain_presets.is_empty() {
                    ui.label("No presets saved yet.");
                }
                let names = self.chain_presets.clone();
                for name in names {
                    if ui.button(&name).clicked() {
                        match load_chain_preset(&name) {
                            Ok(preset) => {
                                for (slot, loaded) in inserts.iter_mut().zip(preset) {
                                    *slot = loaded;
                                }
                                changed = true;
                                preset_loaded = true;
                                self.status = format!("Loaded chain preset '{name}'.");
                            }
                            Err(error) => self.status = error,
                        }
                        ui.close();
                    }
                }
            });
        });
        {
            ui.horizontal(|ui| {
                if ui.button("Add insert effect").clicked() {
                    if let Some(slot_id) = inserts.iter().position(|slot| slot.name.is_empty()) {
                        self.plugin_selector_target = Some(PluginTarget::Effect {
                            channel,
                            slot: slot_id,
                        });
                    } else {
                        self.status = "All insert slots are occupied.".to_owned();
                    }
                }
                if ui.button("Choose plugin…").clicked() {
                    self.plugin_selector_target = Some(PluginTarget::Effect { channel, slot: 0 });
                }
            });
            for (slot_id, slot) in inserts.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let name = if slot.name.is_empty() {
                        "Empty"
                    } else {
                        &slot.name
                    };
                    if ui.button(format!("{} · {name}", slot_id + 1)).clicked() {
                        self.open_effect_plugins.insert((channel, slot_id));
                    }
                    if ui.small_button("Select…").clicked() {
                        self.plugin_selector_target = Some(PluginTarget::Effect {
                            channel,
                            slot: slot_id,
                        });
                    }
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        if slot.enabled {
                            egui::Color32::from_rgb(60, 120, 110)
                        } else {
                            egui::Color32::from_gray(35)
                        },
                    );
                    paint_icon(
                        ui.painter(),
                        rect.shrink(3.0),
                        Icon::Power,
                        if slot.enabled {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::from_gray(110)
                        },
                    );
                    if slot.locked {
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left() + 2.0, rect.bottom() - 2.0),
                                egui::pos2(rect.right() - 2.0, rect.bottom() - 2.0),
                            ],
                            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(230, 178, 92)),
                        );
                    }
                    tooltip(ui, &response, "Power. Shift+click locks, ctrl+click solos");
                    if response.clicked() {
                        if self.mod_shift {
                            slot.locked = !slot.locked;
                        } else if self.mod_ctrl {
                            // Ctrl+click solos this insert, muting the other
                            // inserts on the same channel.
                            slot.solo = !slot.solo;
                        } else {
                            // The lock only shields the slot from global
                            // operations; a direct click still bypasses it.
                            slot.enabled = !slot.enabled;
                        }
                        changed = true;
                    }
                    if slot.enabled && ui.small_button("Remove").clicked() {
                        slot.enabled = false;
                        slot.name.clear();
                        changed = true;
                    }
                });
            }
        }
        if let Some(name) = save_preset_request {
            // External plug-ins hold their parameters in the hosted instance, so
            // the opaque state has to be captured before writing the chain out.
            self.capture_plugin_states();
            let slots: &[InsertFxSlot] = if is_master {
                &self.project.master.inserts
            } else {
                &self.project.lanes[channel].inserts
            };
            match save_chain_preset(&name, slots) {
                Ok(name) => {
                    self.status = format!("Saved chain preset '{name}'.");
                    self.chain_presets = list_chain_presets();
                }
                Err(error) => self.status = error,
            }
        }
        if preset_loaded {
            // Re-instantiate the hosted plug-ins so their saved state applies.
            self.restore_hosted_plugins();
        }
        if changed {
            self.sync_project();
        }
    }
}
