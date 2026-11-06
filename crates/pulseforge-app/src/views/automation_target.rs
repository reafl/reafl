//! Picking the parameter that the automation window edits.

use crate::*;

impl PulseForgeApp {
    /// Keeps the remembered instrument, channel and slot indices inside the
    /// current project, so opening the window after a removal cannot panic.
    pub(crate) fn clamp_automation_indices(&mut self) {
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
    }

    /// Draws the instrument / insert-effect picker.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI.
    ///
    /// # Returns
    ///
    /// The selected [`ParameterTarget`], or `None` when the project has
    /// nothing that could be automated yet.
    pub(crate) fn automation_target_selector(&mut self, ui: &mut egui::Ui) -> Option<ParameterTarget> {
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
        target
    }

    /// Draws the button that turns the selected target into a fresh lane.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI.
    /// - `target`: the parameter the new lane should automate.
    pub(crate) fn automation_add_lane_button(&mut self, ui: &mut egui::Ui, target: ParameterTarget) {
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
}

