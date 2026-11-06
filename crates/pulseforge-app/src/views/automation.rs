//! The automation, export and sample-clip windows.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn render_automation_window(&mut self, ui: &mut egui::Ui) {
        // Indices can go stale when an instrument is removed while this window
        // is open, which used to panic here.
        self.automation_instrument_id = self
            .automation_instrument_id
            .min(self.project.instruments.len().saturating_sub(1));
        self.automation_channel_id = self.automation_channel_id.min(self.project.lanes.len());
        self.selected_instrument = self
            .selected_instrument
            .min(self.project.instruments.len().saturating_sub(1));
        self.selected_mixer_channel = self.selected_mixer_channel.min(self.project.lanes.len());
        ui.label("Automate every continuous built-in instrument and insert-effect control.");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.automation_effect_mode, false, "Instrument");
            ui.selectable_value(&mut self.automation_effect_mode, true, "Insert FX");
        });

        let target = if self.automation_effect_mode {
            let master_id = self.project.lanes.len();
            self.automation_channel_id = self.automation_channel_id.min(master_id);
            egui::ComboBox::from_id_salt("automation_channel")
                .selected_text(if self.automation_channel_id == master_id {
                    "Master"
                } else {
                    self.project.lanes[self.automation_channel_id].name.as_str()
                })
                .show_ui(ui, |ui| {
                    for (channel_id, lane) in self.project.lanes.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.automation_channel_id,
                            channel_id,
                            &lane.name,
                        );
                    }

                    ui.selectable_value(&mut self.automation_channel_id, master_id, "Master");
                });
            let slots = if self.automation_channel_id == master_id {
                &self.project.master.inserts
            } else {
                &self.project.lanes[self.automation_channel_id].inserts
            };
            let active_slots: Vec<_> = slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| !slot.name.is_empty())
                .map(|(slot_id, slot)| (slot_id, slot.name.clone()))
                .collect();
            if active_slots.is_empty() {
                ui.label("Add an insert effect to this mixer channel first.");
                None
            } else {
                if !active_slots
                    .iter()
                    .any(|(slot_id, _)| *slot_id == self.automation_slot_id)
                {
                    self.automation_slot_id = active_slots[0].0;
                }
                egui::ComboBox::from_id_salt("automation_fx_slot")
                    .selected_text(
                        active_slots
                            .iter()
                            .find(|(slot_id, _)| *slot_id == self.automation_slot_id)
                            .map_or("FX", |(_, name)| name.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        for (slot_id, name) in &active_slots {
                            ui.selectable_value(&mut self.automation_slot_id, *slot_id, name);
                        }
                    });
                let slot_name = &slots[self.automation_slot_id].name;
                let parameters = effect_parameter_options(slot_name);
                if !parameters.contains(&self.automation_effect_parameter) {
                    self.automation_effect_parameter = parameters[0];
                }
                egui::ComboBox::from_id_salt("automation_fx_parameter")
                    .selected_text(self.automation_effect_parameter.label())
                    .show_ui(ui, |ui| {
                        for parameter in parameters {
                            ui.selectable_value(
                                &mut self.automation_effect_parameter,
                                parameter,
                                parameter.label(),
                            );
                        }
                    });
                Some(ParameterTarget::InsertEffect {
                    channel_id: self.automation_channel_id,
                    slot_id: self.automation_slot_id,
                    parameter: self.automation_effect_parameter,
                })
            }
        } else {
            if self.project.instruments.is_empty() {
                ui.label("Add an instrument first.");
                None
            } else {
                self.automation_instrument_id = self
                    .automation_instrument_id
                    .min(self.project.instruments.len() - 1);
                egui::ComboBox::from_id_salt("automation_instrument")
                    .selected_text(&self.project.instruments[self.automation_instrument_id].name)
                    .show_ui(ui, |ui| {
                        for (instrument_id, instrument) in
                            self.project.instruments.iter().enumerate()
                        {
                            ui.selectable_value(
                                &mut self.automation_instrument_id,
                                instrument_id,
                                &instrument.name,
                            );
                        }
                    });
                let synth_kind = self.project.instruments[self.automation_instrument_id].synth_kind;
                let parameters = instrument_parameter_options(synth_kind);
                if !parameters.contains(&self.automation_instrument_parameter) {
                    self.automation_instrument_parameter = parameters[0];
                }
                egui::ComboBox::from_id_salt("automation_instrument_parameter")
                    .selected_text(self.automation_instrument_parameter.label())
                    .show_ui(ui, |ui| {
                        for parameter in parameters {
                            ui.selectable_value(
                                &mut self.automation_instrument_parameter,
                                parameter,
                                parameter.label(),
                            );
                        }
                    });
                Some(ParameterTarget::Instrument {
                    instrument_id: self.automation_instrument_id,
                    parameter: self.automation_instrument_parameter,
                })
            }
        };

        if let Some(target) = target {
            let target_label = parameter_target_label(&self.project, target);
            if ui.button("Add automation lane").clicked()
                && let Some(base_value) = self.project.parameter_value(target)
            {
                let base = target.normalize_value(base_value);
                self.project.automation_tracks.push(AutomationTrack {
                    name: target_label.clone(),
                    target,
                    start_step: 0,
                    length_steps: 64,
                    points: vec![
                        AutomationPoint {
                            step: 0,
                            value: base,
                            curve: AutomationCurve::Linear,
                        },
                        AutomationPoint {
                            step: 16,
                            value: base,
                            curve: AutomationCurve::Linear,
                        },
                    ],
                    modulation_points: Vec::new(),
                    enabled: true,
                    source: project::AutomationSource::Curve,
                });
                self.sync_project();
            }
        }

        ui.separator();
        // Quick formula tool: an expression is sampled into a real curve lane,
        // so the engine needs no scripting path of its own.
        ui.horizontal(|ui| {
            ui.label("Formula");
            ui.add(
                egui::TextEdit::singleline(&mut self.formula_text)
                    .desired_width(260.0)
                    .hint_text("0.5 + 0.4 * sin(2 * pi * x)"),
            );
        });
        match pulseforge_core::expr::Expression::parse(&self.formula_text) {
            Ok(expression) => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "x=0.25 \u{2192} {:.3} \u{00B7} x=0.5 \u{2192} {:.3} \u{00B7} x=0.75 \u{2192} {:.3}",
                            expression.evaluate(0.25),
                            expression.evaluate(0.5),
                            expression.evaluate(0.75)
                        ))
                        .small()
                        .weak(),
                    );
                    if ui.button("Add formula lane").clicked() {
                        self.add_formula_automation(&expression);
                    }
                });
            }
            Err(error) => {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("formula: {error}"));
            }
        }

        ui.separator();
        let mut changed = false;
        let mut remove_track = None;
        let target_labels: Vec<_> = self
            .project
            .automation_tracks
            .iter()
            .map(|track| parameter_target_label(&self.project, track.target))
            .collect();
        let snap_ticks = self.project.snap_ticks;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (track_id, track) in self.project.automation_tracks.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut track.enabled, "").changed();
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut track.name).desired_width(180.0))
                        .changed();
                    ui.label(&target_labels[track_id]);
                    if ui.small_button("Remove lane").clicked() {
                        remove_track = Some(track_id);
                    }
                });
                let source_label = match track.source {
                    project::AutomationSource::Curve => "Curve",
                    project::AutomationSource::RelativeCurve => "Relative curve",
                    project::AutomationSource::MidiEnvelope { .. } => "MIDI envelope",
                    project::AutomationSource::Lfo { .. } => "LFO",
                    project::AutomationSource::SampleAndHold { .. } => "Sample & hold",
                    project::AutomationSource::Perlin { .. } => "Perlin",
                };
                egui::ComboBox::from_id_salt(("automation_source", track_id))
                    .selected_text(source_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::Curve),
                                "Curve",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::Curve;
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(
                                    track.source,
                                    project::AutomationSource::RelativeCurve
                                ),
                                "Relative curve",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::RelativeCurve;
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(
                                    track.source,
                                    project::AutomationSource::MidiEnvelope { .. }
                                ),
                                "MIDI envelope",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::MidiEnvelope {
                                instrument_id: 0,
                                attack: project::TimeValue {
                                    amount: 5.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                decay: project::TimeValue {
                                    amount: 180.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                sustain: 0.7,
                                release: project::TimeValue {
                                    amount: 250.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                base: 0.0,
                                depth: 1.0,
                            };
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::Lfo { .. }),
                                "LFO",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::Lfo {
                                shape: project::LfoShape::Sine,
                                rate: project::TimeValue {
                                    amount: 500.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                retrigger: true,
                                base: 0.5,
                                depth: 0.5,
                            };
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(
                                    track.source,
                                    project::AutomationSource::SampleAndHold { .. }
                                ),
                                "Sample & hold",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::SampleAndHold {
                                seed: 1,
                                rate: project::TimeValue {
                                    amount: 4.0,
                                    unit: project::TimeUnit::Steps,
                                },
                                base: 0.5,
                                depth: 0.5,
                            };
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::Perlin { .. }),
                                "Perlin noise",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::Perlin {
                                seed: 1,
                                rate: project::TimeValue {
                                    amount: 8.0,
                                    unit: project::TimeUnit::Steps,
                                },
                                octaves: 4,
                                base: 0.5,
                                depth: 0.5,
                            };
                            changed = true;
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("Clip");
                    let old_start = track.start_step;
                    let start_changed = ui
                        .add(
                            egui::DragValue::new(&mut track.start_step)
                                .range(0..=u64::MAX)
                                .prefix("Start "),
                        )
                        .changed();
                    if start_changed {
                        track.start_step = snap_arrangement_step(track.start_step, snap_ticks);
                    }
                    if track.start_step != old_start {
                        let delta = track.start_step as i128 - old_start as i128;
                        for point in track.points.iter_mut().chain(&mut track.modulation_points) {
                            point.step = (point.step as i128 + delta)
                                .clamp(track.start_step as i128, u64::MAX as i128)
                                as u64;
                        }
                        changed = true;
                    }
                    let length_changed = ui
                        .add(
                            egui::DragValue::new(&mut track.length_steps)
                                .range(1..=u64::MAX)
                                .prefix("Length "),
                        )
                        .changed();
                    if length_changed {
                        let snap_steps = snap_ticks.div_ceil(project::TICKS_PER_STEP).max(1);
                        track.length_steps = track.length_steps.saturating_add(snap_steps / 2)
                            / snap_steps
                            * snap_steps;
                        track.length_steps = track.length_steps.max(snap_steps);
                        changed = true;
                    }
                    let clip_end = track.start_step.saturating_add(track.length_steps);
                    for point in track.points.iter_mut().chain(&mut track.modulation_points) {
                        point.step = point.step.min(clip_end.saturating_sub(1));
                    }
                });
                match &mut track.source {
                    project::AutomationSource::Curve | project::AutomationSource::RelativeCurve => {}
                    project::AutomationSource::MidiEnvelope {
                        instrument_id,
                        attack,
                        decay,
                        sustain,
                        release,
                        base,
                        depth,
                    } => {
                        if !self.project.instruments.is_empty() {
                            *instrument_id =
                                (*instrument_id).min(self.project.instruments.len() - 1);
                            egui::ComboBox::from_id_salt(("automation_env_instrument", track_id))
                                .selected_text(&self.project.instruments[*instrument_id].name)
                                .show_ui(ui, |ui| {
                                    for (id, instrument) in
                                        self.project.instruments.iter().enumerate()
                                    {
                                        if ui
                                            .selectable_value(instrument_id, id, &instrument.name)
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                    }
                                });
                        }
                        ui.horizontal(|ui| {
                            changed |= edit_time_value(ui, track_id, "Attack", attack);
                            changed |= edit_time_value(ui, track_id, "Decay", decay);
                            changed |= lane_parameter_slider(
                                ui,
                                "Sustain",
                                sustain,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Sustain,
                                },
                            );
                            changed |= edit_time_value(ui, track_id, "Release", release);
                            changed |= lane_parameter_slider(
                                ui,
                                "Base",
                                base,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Base,
                                },
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Depth",
                                depth,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Depth,
                                },
                            );
                        });
                    }
                    project::AutomationSource::Lfo {
                        shape,
                        rate,
                        retrigger,
                        base,
                        depth,
                    } => {
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt(("automation_lfo_shape", track_id))
                                .selected_text(format!("{shape:?}"))
                                .show_ui(ui, |ui| {
                                    // Sample & hold is its own source now, so the LFO
                                    // shape list no longer offers it.
                                    for candidate in [
                                        project::LfoShape::Sine,
                                        project::LfoShape::Triangle,
                                        project::LfoShape::Saw,
                                        project::LfoShape::Square,
                                    ] {
                                        if ui
                                            .selectable_value(
                                                shape,
                                                candidate,
                                                format!("{candidate:?}"),
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                    }
                                });
                            // The shape is an enum, automated as a plain integer.
                            automation_lane_button(
                                ui,
                                "LFO shape",
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Shape,
                                },
                            );
                        });
                        ui.horizontal(|ui| {
                            changed |= edit_time_value_automated(
                                ui,
                                track_id,
                                "Rate",
                                rate,
                                Some(ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::RateAmount,
                                }),
                            );
                            changed |= ui.checkbox(retrigger, "Retrigger").changed();
                            changed |= lane_parameter_slider(
                                ui,
                                "Base",
                                base,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Base,
                                },
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Depth",
                                depth,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Depth,
                                },
                            );
                        });
                    }
                    project::AutomationSource::SampleAndHold {
                        seed,
                        rate,
                        base,
                        depth,
                    } => {
                        ui.horizontal(|ui| {
                            changed |= edit_time_value_automated(
                                ui,
                                track_id,
                                "Rate",
                                rate,
                                Some(ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::RateAmount,
                                }),
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Base",
                                base,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Base,
                                },
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Depth",
                                depth,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Depth,
                                },
                            );
                            ui.label("Seed");
                            changed |= ui
                                .add(egui::DragValue::new(seed).range(0..=u32::MAX))
                                .changed();
                        });
                    }
                    project::AutomationSource::Perlin {
                        seed,
                        rate,
                        octaves,
                        base,
                        depth,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label("Seed");
                            changed |= ui
                                .add(egui::DragValue::new(seed).range(0..=u32::MAX))
                                .changed();
                            changed |= edit_time_value_automated(
                                ui,
                                track_id,
                                "Rate",
                                rate,
                                Some(ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::RateAmount,
                                }),
                            );
                            let mut octave_count = *octaves as f32;
                            if lane_parameter_slider(
                                ui,
                                "Octaves",
                                &mut octave_count,
                                1.0..=16.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Octaves,
                                },
                            ) {
                                *octaves = octave_count.round().clamp(1.0, 16.0) as u32;
                            }
                            changed |= lane_parameter_slider(
                                ui,
                                "Base",
                                base,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Base,
                                },
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Depth",
                                depth,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Depth,
                                },
                            );
                        });
                    }
                }
                if track.source.is_relative() {
                    ui.label("Draw offsets from -1 to 1; they are added to the parameter.");
                } else if !track.source.edits_points() {
                    ui.label("Draw a 0-1 modulation multiplier directly over the lane.");
                }
                ui.separator();
            }
        });
        if let Some(track_id) = remove_track {
            self.project.automation_tracks.remove(track_id);
            changed = true;
        }
        if changed {
            self.sync_project();
        }
    }
}

impl PulseForgeApp {
    /// Samples a parsed formula into a curve lane on the current automation
    /// target, so a formula behaves like any hand-drawn lane afterwards.
    pub(crate) fn add_formula_automation(&mut self, expression: &pulseforge_core::expr::Expression) {
        const LENGTH_STEPS: u64 = 64;
        const POINT_COUNT: u64 = 32;
        let target = if self.automation_effect_mode {
            ParameterTarget::InsertEffect {
                channel_id: self.automation_channel_id,
                slot_id: self.automation_slot_id,
                parameter: self.automation_effect_parameter,
            }
        } else {
            ParameterTarget::Instrument {
                instrument_id: self.automation_instrument_id,
                parameter: self.automation_instrument_parameter,
            }
        };
        let points: Vec<AutomationPoint> = (0..=POINT_COUNT)
            .map(|index| AutomationPoint {
                step: LENGTH_STEPS * index / POINT_COUNT,
                value: expression
                    .evaluate(index as f64 / POINT_COUNT as f64)
                    .clamp(0.0, 1.0) as f32,
                curve: AutomationCurve::Linear,
            })
            .collect();
        let label = parameter_target_label(&self.project, target);
        self.push_history();
        self.project.automation_tracks.push(AutomationTrack {
            name: format!("{label} (formula)"),
            target,
            start_step: 0,
            length_steps: LENGTH_STEPS,
            points,
            modulation_points: Vec::new(),
            enabled: true,
            source: project::AutomationSource::Curve,
        });
        self.sync_project();
        self.status = "Formula sampled into a new automation lane.".to_owned();
    }
}

