//! The instrument plug-in window and synth controls.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn render_instrument_plugin(&mut self, instrument_id: usize, ui: &mut egui::Ui) {
        let name = self
            .project
            .tracks
            .get(instrument_id)
            .map_or("Plug-in".to_owned(), |track| track.name.clone());
        if self.plugin_window_header(ui, PluginTarget::Instrument(instrument_id), &name) {
            self.render_plugin_routing(ui, instrument_id);
            if ui.input(|input| input.pointer.any_released()) {
                self.sync_project();
            }
            return;
        }
        let show_waveform = self
            .project
            .tracks
            .get(instrument_id)
            .and_then(|track| track.generator())
            .is_some_and(|generator| generator.kind.sample().is_some());
        let waveform_rect = show_waveform.then(|| {
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover())
                .0
        });
        if let Some(rect) = waveform_rect {
            self.draw_sample_waveform(ui.painter_at(rect), rect, instrument_id);
        }
        let Some(instrument) = self
            .project
            .tracks
            .get_mut(instrument_id)
            .and_then(Track::generator_mut)
        else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Instrument no longer exists.");
            return;
        };
        let mut changed = false;
        let mut load_sample = false;
        ui.horizontal(|ui| {
            ui.heading(&name);
            let kind_label = instrument
                .kind
                .synth_kind()
                .map_or("Sample", synth_kind_label);
            ui.label(format!("· {kind_label}"));
            if ui.button("Select plugin...").clicked() {
                self.plugin_selector_target = Some(PluginTarget::Instrument(instrument_id));
            }
        });
        ui.separator();

        match instrument.kind {
            GeneratorKind::Synth(SynthKind::External) => {
                self.render_external_plugin_wrapper(ui, instrument_id);
            }
            GeneratorKind::Sample(_) => {
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
                    let mut selected = instrument.kind.sample().unwrap_or_default();
                    egui::ComboBox::from_id_salt(("default_sample", instrument_id))
                        .selected_text(default_sample_label(selected))
                        .show_ui(ui, |ui| {
                            for (sample, label) in [
                                (DefaultSample::None, "None"),
                                (DefaultSample::Kick, "Kick"),
                                (DefaultSample::Snare, "Snare"),
                                (DefaultSample::HiHat, "Hi-hat"),
                            ] {
                                changed |=
                                    ui.selectable_value(&mut selected, sample, label).changed();
                            }
                        });
                    if instrument.kind.sample().unwrap_or_default() != selected {
                        instrument.kind = GeneratorKind::Sample(selected);
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    changed |= Knob::new("LEVEL", 0.0, 1.0, "")
                        .with_default(0.8)
                        .automates(ParameterTarget::Track {
                            track_id: instrument_id,
                            parameter: TrackParameter::Generator(InstrumentParameter::Gain),
                        })
                        .show(ui, &mut instrument.gain);
                    changed |= Knob::new("PAN", -1.0, 1.0, "")
                        .with_default(0.0)
                        .automates(ParameterTarget::Track {
                            track_id: instrument_id,
                            parameter: TrackParameter::Generator(InstrumentParameter::Pan),
                        })
                        .show(ui, &mut instrument.pan);
                });
            }
            GeneratorKind::Synth(kind) => {
                ui.horizontal(|ui| {
                    changed |= Knob::new("LEVEL", 0.0, 1.0, "")
                        .with_default(0.8)
                        .automates(ParameterTarget::Track {
                            track_id: instrument_id,
                            parameter: TrackParameter::Generator(InstrumentParameter::Gain),
                        })
                        .show(ui, &mut instrument.gain);
                    changed |= Knob::new("PAN", -1.0, 1.0, "")
                        .with_default(0.0)
                        .automates(ParameterTarget::Track {
                            track_id: instrument_id,
                            parameter: TrackParameter::Generator(InstrumentParameter::Pan),
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
                            ParameterTarget::Track {
                                track_id: instrument_id,
                                parameter: TrackParameter::Generator(
                                    InstrumentParameter::SynthNote,
                                ),
                            },
                        ) {
                            instrument.synth_note = note.round().clamp(0.0, 127.0) as u8;
                        }
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt(("signal_waveform_editor", instrument_id))
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
                                ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(
                                        InstrumentParameter::Waveform,
                                    ),
                                },
                            );
                        });
                    }
                    SynthKind::Ms20 => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("CUTOFF", 40.0, 16_000.0, " Hz")
                                .with_default(1_800.0)
                                .automates(ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(
                                        InstrumentParameter::CutoffHz,
                                    ),
                                })
                                .show(ui, &mut instrument.ms20_cutoff_hz);
                            changed |= Knob::new("RESONANCE", 0.0, 0.9, "")
                                .with_default(0.2)
                                .automates(ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(
                                        InstrumentParameter::Resonance,
                                    ),
                                })
                                .show(ui, &mut instrument.ms20_resonance);
                        });
                    }
                    SynthKind::JunoPoly => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("CUTOFF", 40.0, 16_000.0, " Hz")
                                .with_default(2_400.0)
                                .automates(ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(
                                        InstrumentParameter::CutoffHz,
                                    ),
                                })
                                .show(ui, &mut instrument.juno_cutoff_hz);
                            changed |= Knob::new("RESONANCE", 0.0, 0.9, "")
                                .with_default(0.15)
                                .automates(ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(
                                        InstrumentParameter::Resonance,
                                    ),
                                })
                                .show(ui, &mut instrument.juno_resonance);
                            changed |= Knob::new("PULSE", 0.05, 0.95, "")
                                .with_default(0.5)
                                .automates(ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(
                                        InstrumentParameter::PulseWidth,
                                    ),
                                })
                                .show(ui, &mut instrument.juno_pulse_width);
                            changed |= Knob::new("SUB", 0.0, 1.0, "")
                                .with_default(0.3)
                                .automates(ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(
                                        InstrumentParameter::SubLevel,
                                    ),
                                })
                                .show(ui, &mut instrument.juno_sub_level);
                        });
                    }
                    SynthKind::M1Workstation => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("TONE", 0.0, 1.0, "")
                                .with_default(0.55)
                                .automates(ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(InstrumentParameter::Tone),
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
                                        .automates(ParameterTarget::Track {
                                            track_id: instrument_id,
                                            parameter: TrackParameter::Generator(
                                                InstrumentParameter::Decay,
                                            ),
                                        })
                                        .show(ui, &mut instrument.m1_decay_steps);
                                }
                                project::TimeUnit::Milliseconds => {
                                    let mut decay_ms = instrument.m1_decay * 1_000.0;
                                    let decay_changed = Knob::new("DECAY", 50.0, 4_000.0, " ms")
                                        .with_default(1_200.0)
                                        .automates(ParameterTarget::Track {
                                            track_id: instrument_id,
                                            parameter: TrackParameter::Generator(
                                                InstrumentParameter::Decay,
                                            ),
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
                                .automates(ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(
                                        InstrumentParameter::FmRatio,
                                    ),
                                })
                                .show(ui, &mut instrument.fm_ratio);
                            changed |= Knob::new("FM INDEX", 0.0, 1.0, "")
                                .with_default(0.35)
                                .automates(ParameterTarget::Track {
                                    track_id: instrument_id,
                                    parameter: TrackParameter::Generator(
                                        InstrumentParameter::FmIndex,
                                    ),
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
}
