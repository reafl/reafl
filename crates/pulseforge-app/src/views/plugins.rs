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
                    let mut voices = f32::from(instrument.voices.voice_count);
                    let voices_target = ParameterTarget::Instrument {
                        instrument_id,
                        parameter: InstrumentParameter::VoiceCount,
                    };
                    let voices_response = ui.add(
                        egui::DragValue::new(&mut voices)
                            .range(1.0..=64.0)
                            .speed(0.2),
                    );
                    // Polyphony is a synth parameter, so it can be automated
                    // like the filter or the arpeggiator.
                    automation_context_menu(
                        &voices_response,
                        &format!("Voices · {voices:.0}"),
                        voices_target,
                    );
                    if voices_response.changed() {
                        instrument.voices.voice_count = voices.round().clamp(1.0, 64.0) as u8;
                    }
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
}

impl PulseForgeApp {}
