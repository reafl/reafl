//! Instrument and effect plug-in windows.

use crate::*;

impl PulseForgeApp {
    /// Shared plug-in window header. Returns true when the settings tab is
    /// active, which means the window shows routing/voice options instead of
    /// the plug-in interface.
    pub(crate) fn plugin_window_header(
        &mut self,
        ui: &mut egui::Ui,
        target: PluginTarget,
        title: &str,
    ) -> bool {
        let settings_open = self.plugin_settings_tab.contains(&target);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.selectable_label(settings_open, "Settings").clicked() {
                    if settings_open {
                        self.plugin_settings_tab.remove(&target);
                    } else {
                        self.plugin_settings_tab.insert(target);
                    }
                }
                if ui.selectable_label(!settings_open, "Plug-in").clicked() {
                    self.plugin_settings_tab.remove(&target);
                }
            });
        });
        ui.separator();
        self.plugin_settings_tab.contains(&target)
    }

    /// Plug-in settings tab: routing, voice count and the arpeggiator.
    ///
    /// The two groups sit side by side so the page stays short instead of
    /// turning into a long vertical strip.
    pub(crate) fn render_plugin_routing(&mut self, ui: &mut egui::Ui, instrument_id: usize) {
        let Some(instrument) = self.project.instruments.get_mut(instrument_id) else {
            return;
        };
        ui.columns(2, |columns| {
            let left = &mut columns[0];
            left.label(egui::RichText::new("Routing").strong());
            egui::Grid::new(("plugin_routing", instrument_id))
                .num_columns(2)
                .show(left, |ui| {
                    ui.label("MIDI in channel");
                    ui.add(
                        egui::DragValue::new(&mut instrument.voices.midi_in_channel).range(0..=15),
                    );
                    ui.end_row();
                    ui.label("MIDI out channel");
                    ui.add(
                        egui::DragValue::new(&mut instrument.voices.midi_out_channel).range(0..=15),
                    );
                    ui.end_row();
                    ui.label("Audio in");
                    ui.add(egui::DragValue::new(&mut instrument.voices.audio_in).range(0..=15));
                    ui.end_row();
                    ui.label("Audio out");
                    ui.add(egui::DragValue::new(&mut instrument.voices.audio_out).range(0..=15));
                    ui.end_row();
                    ui.label("Voices");
                    ui.add(egui::DragValue::new(&mut instrument.voices.voice_count).range(1..=64));
                    ui.end_row();
                });

            let right = &mut columns[1];
            right.label(egui::RichText::new("Arpeggiator").strong());
            right.checkbox(&mut instrument.arpeggio.enabled, "Enabled");
            egui::Grid::new(("plugin_arp", instrument_id))
                .num_columns(2)
                .show(right, |ui| {
                    ui.label("Mode");
                    let arp_mode = ParameterTarget::Instrument {
                        instrument_id,
                        parameter: InstrumentParameter::ArpMode,
                    };
                    // The combo and its automation button share one grid cell.
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt(("arp_mode", instrument_id))
                            .selected_text(instrument.arpeggio.mode.label())
                            .show_ui(ui, |ui| {
                                for mode in project::ArpMode::ALL {
                                    ui.selectable_value(
                                        &mut instrument.arpeggio.mode,
                                        mode,
                                        mode.label(),
                                    );
                                }
                            });
                        automation_lane_button(
                            ui,
                            &format!("arp mode · {}", instrument.arpeggio.mode.label()),
                            arp_mode,
                        );
                    });
                    ui.end_row();
                    ui.label("Gate");
                    let mut gate = instrument.arpeggio.gate;
                    let gate_edit = ValueControl::new("", &mut gate, 0.05..=1.0)
                        .automates(ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::ArpGate,
                        })
                        .show(ui);
                    if gate_edit.changed {
                        instrument.arpeggio.gate = gate;
                    }
                    ui.end_row();
                    ui.label("Octaves");
                    let mut octaves = f32::from(instrument.arpeggio.octaves);
                    if lane_parameter_slider(
                        ui,
                        "",
                        &mut octaves,
                        1.0..=8.0,
                        ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::ArpOctaves,
                        },
                    ) {
                        instrument.arpeggio.octaves = octaves.round().clamp(1.0, 8.0) as u8;
                    }
                    ui.end_row();
                });
            let mut arp_rate = instrument.arpeggio.rate;
            if edit_time_value_automated(
                right,
                instrument_id,
                "Rate",
                &mut arp_rate,
                Some(ParameterTarget::Instrument {
                    instrument_id,
                    parameter: InstrumentParameter::ArpRate,
                }),
            ) {
                instrument.arpeggio.rate = arp_rate;
            }
        });
    }

    pub(crate) fn render_instrument_plugin(&mut self, instrument_id: usize, ui: &mut egui::Ui) {
        let name = self
            .project
            .instruments
            .get(instrument_id)
            .map_or("Plug-in".to_owned(), |instrument| instrument.name.clone());
        if self.plugin_window_header(ui, PluginTarget::Instrument(instrument_id), &name) {
            self.render_plugin_routing(ui, instrument_id);
            if ui.input(|input| input.pointer.any_released()) {
                self.sync_project();
            }
            return;
        }
        let show_waveform = self
            .project
            .instruments
            .get(instrument_id)
            .is_some_and(|instrument| instrument.synth_kind == SynthKind::SamplePlayer);
        let waveform_rect = show_waveform.then(|| {
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover())
                .0
        });
        if let Some(rect) = waveform_rect {
            self.draw_sample_waveform(ui.painter_at(rect), rect, instrument_id);
        }
        let Some(instrument) = self.project.instruments.get_mut(instrument_id) else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Instrument no longer exists.");
            return;
        };
        let mut changed = false;
        let mut load_sample = false;
        ui.horizontal(|ui| {
            ui.heading(&instrument.name);
            ui.label(format!("· {}", synth_kind_label(instrument.synth_kind)));
            if ui.button("Select plugin...").clicked() {
                self.plugin_selector_target = Some(PluginTarget::Instrument(instrument_id));
            }
        });
        ui.separator();

        match instrument.synth_kind {
            SynthKind::External => {
                self.render_external_plugin_wrapper(ui, instrument_id);
            }
            SynthKind::SamplePlayer => {
                ui.heading("Sample Player");
                ui.label("Plays the instrument's sample on sequencer and MIDI triggers.");
                if waveform_rect.is_some() {
                    ui.separator();
                }
                let mut path = instrument.sample_path.clone();
                ui.horizontal(|ui| {
                    ui.label("WAV");
                    ui.add(
                        egui::TextEdit::singleline(&mut path)
                            .desired_width(330.0)
                            .hint_text("Path to a WAV sample"),
                    );
                    if ui.button("Load sample").clicked() {
                        instrument.sample_path = path.clone();
                        load_sample = true;
                        changed = true;
                    }
                });
                if path != instrument.sample_path {
                    instrument.sample_path = path;
                    changed = true;
                }
                ui.horizontal(|ui| {
                    ui.label("Default sound");
                    egui::ComboBox::from_id_salt(("default_sample", instrument_id))
                        .selected_text(default_sample_label(instrument.default_sample))
                        .show_ui(ui, |ui| {
                            for (sample, label) in [
                                (DefaultSample::None, "None"),
                                (DefaultSample::Kick, "Kick"),
                                (DefaultSample::Snare, "Snare"),
                                (DefaultSample::HiHat, "Hi-hat"),
                            ] {
                                changed |= ui
                                    .selectable_value(&mut instrument.default_sample, sample, label)
                                    .changed();
                            }
                        });
                });
                ui.horizontal(|ui| {
                    changed |= Knob::new("LEVEL", 0.0, 1.0, "")
                        .with_default(0.8)
                        .automates(ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::Gain,
                        })
                        .show(ui, &mut instrument.gain);
                    changed |= Knob::new("PAN", -1.0, 1.0, "")
                        .with_default(0.0)
                        .automates(ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::Pan,
                        })
                        .show(ui, &mut instrument.pan);
                });
            }
            kind => {
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut instrument.synth_enabled, "Enabled")
                        .changed();
                    changed |= Knob::new("LEVEL", 0.0, 1.0, "")
                        .with_default(0.8)
                        .automates(ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::Gain,
                        })
                        .show(ui, &mut instrument.gain);
                    changed |= Knob::new("PAN", -1.0, 1.0, "")
                        .with_default(0.0)
                        .automates(ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::Pan,
                        })
                        .show(ui, &mut instrument.pan);
                });
                match kind {
                    // Handled by the wrapper above; kept for exhaustiveness.
                    SynthKind::External => {}
                    SynthKind::SignalGenerator => {
                        let mut note = f32::from(instrument.synth_note);
                        if lane_parameter_slider(
                            ui,
                            "MIDI note",
                            &mut note,
                            0.0..=127.0,
                            ParameterTarget::Instrument {
                                instrument_id,
                                parameter: InstrumentParameter::SynthNote,
                            },
                        ) {
                            instrument.synth_note = note.round().clamp(0.0, 127.0) as u8;
                        }
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt((
                                "signal_waveform_editor",
                                instrument_id,
                            ))
                            .selected_text(waveform_label(instrument.signal_waveform))
                            .show_ui(ui, |ui| {
                                for (waveform, label) in [
                                    (SignalWaveform::Sine, "Sine"),
                                    (SignalWaveform::Triangle, "Triangle"),
                                    (SignalWaveform::Saw, "Saw"),
                                    (SignalWaveform::Square, "Square"),
                                ] {
                                    changed |= ui
                                        .selectable_value(
                                            &mut instrument.signal_waveform,
                                            waveform,
                                            label,
                                        )
                                        .changed();
                                }
                            });
                            // The waveform is an enum, automated as a plain integer.
                            automation_lane_button(
                                ui,
                                &format!(
                                    "waveform · {}",
                                    waveform_label(instrument.signal_waveform)
                                ),
                                ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::Waveform,
                                },
                            );
                        });
                    }
                    SynthKind::Ms20 => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("CUTOFF", 40.0, 16_000.0, " Hz")
                                .with_default(1_800.0)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::CutoffHz,
                                })
                                .show(ui, &mut instrument.ms20_cutoff_hz);
                            changed |= Knob::new("RESONANCE", 0.0, 0.9, "")
                                .with_default(0.2)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::Resonance,
                                })
                                .show(ui, &mut instrument.ms20_resonance);
                        });
                    }
                    SynthKind::JunoPoly => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("CUTOFF", 40.0, 16_000.0, " Hz")
                                .with_default(2_400.0)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::CutoffHz,
                                })
                                .show(ui, &mut instrument.juno_cutoff_hz);
                            changed |= Knob::new("RESONANCE", 0.0, 0.9, "")
                                .with_default(0.15)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::Resonance,
                                })
                                .show(ui, &mut instrument.juno_resonance);
                            changed |= Knob::new("PULSE", 0.05, 0.95, "")
                                .with_default(0.5)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::PulseWidth,
                                })
                                .show(ui, &mut instrument.juno_pulse_width);
                            changed |= Knob::new("SUB", 0.0, 1.0, "")
                                .with_default(0.3)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::SubLevel,
                                })
                                .show(ui, &mut instrument.juno_sub_level);
                        });
                    }
                    SynthKind::M1Workstation => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("TONE", 0.0, 1.0, "")
                                .with_default(0.55)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::Tone,
                                })
                                .show(ui, &mut instrument.m1_tone);
                            egui::ComboBox::from_id_salt(("m1_decay_unit", instrument_id))
                                .selected_text(match instrument.m1_decay_unit {
                                    project::TimeUnit::Steps => "Steps",
                                    project::TimeUnit::Milliseconds => "ms",
                                })
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut instrument.m1_decay_unit,
                                            project::TimeUnit::Steps,
                                            "Steps",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut instrument.m1_decay_unit,
                                            project::TimeUnit::Milliseconds,
                                            "Milliseconds",
                                        )
                                        .changed();
                                });
                            match instrument.m1_decay_unit {
                                project::TimeUnit::Steps => {
                                    changed |= Knob::new("DECAY", 0.01, 256.0, " steps")
                                        .with_default(4.0)
                                        .automates(ParameterTarget::Instrument {
                                            instrument_id,
                                            parameter: InstrumentParameter::Decay,
                                        })
                                        .show(ui, &mut instrument.m1_decay_steps);
                                }
                                project::TimeUnit::Milliseconds => {
                                    let mut decay_ms = instrument.m1_decay * 1_000.0;
                                    let decay_changed = Knob::new("DECAY", 50.0, 4_000.0, " ms")
                                        .with_default(1_200.0)
                                        .automates(ParameterTarget::Instrument {
                                            instrument_id,
                                            parameter: InstrumentParameter::Decay,
                                        })
                                        .show(ui, &mut decay_ms);
                                    if decay_changed {
                                        instrument.m1_decay = decay_ms / 1_000.0;
                                        changed = true;
                                    }
                                }
                            }
                        });
                    }
                    SynthKind::FmSynth => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("RATIO", 0.25, 16.0, "")
                                .with_default(2.0)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::FmRatio,
                                })
                                .show(ui, &mut instrument.fm_ratio);
                            changed |= Knob::new("FM INDEX", 0.0, 1.0, "")
                                .with_default(0.35)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::FmIndex,
                                })
                                .show(ui, &mut instrument.fm_index);
                        });
                    }
                    SynthKind::Sine | SynthKind::SamplePlayer => {}
                }
            }
        }
        if changed {
            self.sync_project();
        }
        if load_sample {
            self.load_instrument_sample(instrument_id);
        }
    }

    pub(crate) fn render_effect_plugin(&mut self, channel: usize, slot_id: usize, ui: &mut egui::Ui) {
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
                    channel_id: channel,
                    slot_id,
                    parameter: EffectParameter::Mix,
                };
                let mut mix = target_live_value(ui, mix_target).unwrap_or(effect.mix);
                let mix_response = ui.add(egui::Slider::new(&mut mix, 0.0..=1.0).text("Mix"));
                automation_context_menu(
                    &mix_response,
                    &format!("Mix · {mix:.2}"),
                    mix_target,
                );
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
        let lane_count = self.project.lanes.len();
        let channel_label = if channel == lane_count {
            "MASTER".to_owned()
        } else if let Some(lane) = self.project.lanes.get(channel) {
            lane.name.clone()
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Mixer channel no longer exists.");
            return;
        };
        let slots = if channel == lane_count {
            &mut self.project.master.inserts
        } else if let Some(lane) = self.project.lanes.get_mut(channel) {
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
                let Some(instrument) = self.project.instruments.get(instrument_id) else {
                    ui.colored_label(egui::Color32::LIGHT_RED, "Instrument no longer exists.");
                    return;
                };
                ui.label(format!("Choose a plug-in for {}", instrument.name));
                let current = instrument.synth_kind;
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
                        let instrument = &mut self.project.instruments[instrument_id];
                        instrument.synth_kind = kind;
                        instrument.plugin_path.clear();
                        instrument.enabled = true;
                        instrument.synth_enabled = kind != SynthKind::SamplePlayer;
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
                        .instruments
                        .get(instrument_id)
                        .is_some_and(|instrument| instrument.plugin_path == plugin.path);
                    if ui
                        .selectable_label(active, plugin.label())
                        .on_hover_text(&plugin.path)
                        .clicked()
                    {
                        let host_id = pulseforge_core::hosted::instrument_host_id(instrument_id);
                        let path = plugin.path.clone();
                        self.load_host(host_id, &path, true);
                        let instrument = &mut self.project.instruments[instrument_id];
                        instrument.plugin_path = plugin.path.clone();
                        instrument.name = plugin.name.clone();
                        // An external generator must not keep the built-in voice
                        // it had before, otherwise it renders as that synth.
                        instrument.synth_kind = SynthKind::External;
                        instrument.synth_enabled = true;
                        instrument.enabled = true;
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

