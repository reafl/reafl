//! Picking the parameter that the automation window edits.

use crate::*;

impl ReaflApp {
    /// Keeps the remembered instrument, channel, slot and clip indices inside
    /// the current project, so opening the window after a removal cannot panic.
    pub(crate) fn clamp_automation_indices(&mut self) {
        // Indices can go stale when an instrument is removed while this window
        // is open, which used to panic here.
        self.automation_track_id = self
            .automation_track_id
            .min(self.project.tracks.len().saturating_sub(1));
        self.automation_channel_id = self.automation_channel_id.min(self.project.tracks.len());
        self.automation_mixer_channel_id = self
            .automation_mixer_channel_id
            .min(self.project.tracks.len());
        self.selected_instrument = self
            .selected_instrument
            .min(self.project.tracks.len().saturating_sub(1));
        self.selected_mixer_channel = self.selected_mixer_channel.min(self.project.tracks.len());
    }

    /// Points the picker at a target the user chose elsewhere, so a control's
    /// "Add automation lane" entry opens the window on the same parameter.
    ///
    /// # Parameters
    ///
    /// - `target`: the parameter that was picked in the UI.
    pub(crate) fn focus_automation_target(&mut self, target: ParameterTarget) {
        match target {
            ParameterTarget::Track {
                track_id,
                parameter,
            } => {
                self.automation_target_kind = AutomationTargetKind::Instrument;
                self.automation_track_id = track_id;
                self.automation_track_parameter = parameter;
            }
            ParameterTarget::InsertEffect {
                track_id,
                slot_id,
                parameter,
            } => {
                self.automation_target_kind = AutomationTargetKind::InsertEffect;
                self.automation_channel_id = track_id;
                self.automation_slot_id = slot_id;
                self.automation_effect_parameter = parameter;
            }
            ParameterTarget::Clip {
                track_id,
                clip_id,
                parameter,
            } => {
                self.automation_target_kind = AutomationTargetKind::Clip;
                self.automation_sample_defaults = false;
                self.automation_clip_lane = track_id;
                self.automation_clip_id = clip_id;
                self.automation_clip_parameter = parameter;
            }
            ParameterTarget::SampleDefaults { parameter } => {
                self.automation_target_kind = AutomationTargetKind::Clip;
                self.automation_sample_defaults = true;
                self.automation_clip_parameter = parameter;
            }
            ParameterTarget::PluginParameter { .. }
            | ParameterTarget::AutomationLane { .. }
            | ParameterTarget::Tempo => {}
        }
    }

    /// Draws the mode toggles and the picker for the active mode.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI.
    ///
    /// # Returns
    ///
    /// The selected [`ParameterTarget`], or `None` when the project has
    /// nothing that could be automated yet.
    pub(crate) fn automation_target_selector(
        &mut self,
        ui: &mut egui::Ui,
    ) -> Option<ParameterTarget> {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.automation_target_kind,
                AutomationTargetKind::Instrument,
                "Instrument",
            );
            ui.selectable_value(
                &mut self.automation_target_kind,
                AutomationTargetKind::InsertEffect,
                "Insert FX",
            );
            ui.selectable_value(
                &mut self.automation_target_kind,
                AutomationTargetKind::Mixer,
                "Mixer",
            );
            ui.selectable_value(
                &mut self.automation_target_kind,
                AutomationTargetKind::Clip,
                "Clip",
            );
        });
        match self.automation_target_kind {
            AutomationTargetKind::Instrument => self.pick_instrument_target(ui),
            AutomationTargetKind::InsertEffect => self.pick_insert_target(ui),
            AutomationTargetKind::Mixer => self.pick_mixer_target(ui),
            AutomationTargetKind::Clip => self.pick_clip_target(ui),
        }
    }

    /// The target the picker currently describes, without drawing anything.
    ///
    /// Used by the formula tool, which needs the same target the "Add
    /// automation lane" button would create.
    ///
    /// # Returns
    ///
    /// The remembered target, or `None` when it no longer exists.
    pub(crate) fn selected_automation_target(&self) -> Option<ParameterTarget> {
        let target = match self.automation_target_kind {
            AutomationTargetKind::Instrument => ParameterTarget::Track {
                track_id: self.automation_track_id,
                parameter: self.automation_track_parameter,
            },
            AutomationTargetKind::InsertEffect => ParameterTarget::InsertEffect {
                track_id: self.automation_channel_id,
                slot_id: self.automation_slot_id,
                parameter: self.automation_effect_parameter,
            },
            AutomationTargetKind::Mixer => ParameterTarget::Track {
                track_id: self.automation_mixer_channel_id,
                parameter: TrackParameter::Mixer(self.automation_mixer_parameter),
            },
            AutomationTargetKind::Clip => {
                if self.automation_sample_defaults {
                    ParameterTarget::SampleDefaults {
                        parameter: self.automation_clip_parameter,
                    }
                } else {
                    ParameterTarget::Clip {
                        track_id: self.automation_clip_lane,
                        clip_id: self.automation_clip_id,
                        parameter: self.automation_clip_parameter,
                    }
                }
            }
        };
        self.project.parameter_value(target)?;
        Some(target)
    }

    /// Draws the button that turns the selected target into a fresh lane.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI.
    /// - `target`: the parameter the new lane should automate.
    pub(crate) fn automation_add_lane_button(
        &mut self,
        ui: &mut egui::Ui,
        target: ParameterTarget,
    ) {
        let label = parameter_target_label(&self.project, target);
        if ui
            .button(format!("Add automation lane for {label}"))
            .clicked()
        {
            self.create_automation_for_target(target);
        }
    }

    /// Track picker: which track and which of its parameters.
    ///
    /// A folder also lists the child parameters it forwards through
    /// `Track::group_targets`, so one lane can drive a whole submix.
    fn pick_instrument_target(&mut self, ui: &mut egui::Ui) -> Option<ParameterTarget> {
        if self.project.tracks.is_empty() {
            ui.label("Add a track first.");
            return None;
        }
        self.automation_track_id = self.automation_track_id.min(self.project.tracks.len() - 1);
        egui::ComboBox::from_id_salt("automation_track")
            .selected_text(&self.project.tracks[self.automation_track_id].name)
            .show_ui(ui, |ui| {
                for (track_id, track) in self.project.tracks.iter().enumerate() {
                    ui.selectable_value(&mut self.automation_track_id, track_id, &track.name);
                }
            });
        let parameters =
            Track::automation_parameters(&self.project.tracks, self.automation_track_id);
        if parameters.is_empty() {
            ui.label("This track has no automatable parameters.");
            return None;
        }
        if !parameters.contains(&self.automation_track_parameter) {
            self.automation_track_parameter = parameters[0];
        }
        egui::ComboBox::from_id_salt("automation_track_parameter")
            .selected_text(self.automation_track_parameter.label())
            .show_ui(ui, |ui| {
                for parameter in parameters {
                    ui.selectable_value(
                        &mut self.automation_track_parameter,
                        parameter,
                        parameter.label(),
                    );
                }
            });
        self.selected_automation_target()
    }
    /// Insert-effect picker: channel, slot and parameter.
    fn pick_insert_target(&mut self, ui: &mut egui::Ui) -> Option<ParameterTarget> {
        let master_id = self.project.tracks.len();
        self.automation_channel_id = self.automation_channel_id.min(master_id);
        egui::ComboBox::from_id_salt("automation_channel")
            .selected_text(if self.automation_channel_id == master_id {
                "Master"
            } else {
                self.project.tracks[self.automation_channel_id]
                    .name
                    .as_str()
            })
            .show_ui(ui, |ui| {
                for (track_id, lane) in self.project.tracks.iter().enumerate() {
                    ui.selectable_value(&mut self.automation_channel_id, track_id, &lane.name);
                }
                ui.selectable_value(&mut self.automation_channel_id, master_id, "Master");
            });
        let slots = if self.automation_channel_id == master_id {
            &self.project.master.inserts
        } else {
            &self.project.tracks[self.automation_channel_id].inserts
        };
        let active_slots: Vec<_> = slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| !slot.name.is_empty())
            .map(|(slot_id, slot)| (slot_id, slot.name.clone()))
            .collect();
        if active_slots.is_empty() {
            ui.label("Add an insert effect to this mixer channel first.");
            return None;
        }
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
        self.selected_automation_target()
    }

    /// Mixer picker: channel controls, EQ bands and sends.
    fn pick_mixer_target(&mut self, ui: &mut egui::Ui) -> Option<ParameterTarget> {
        let master_id = self.project.tracks.len();
        self.automation_mixer_channel_id = self.automation_mixer_channel_id.min(master_id);
        egui::ComboBox::from_id_salt("automation_mixer_channel")
            .selected_text(if self.automation_mixer_channel_id == master_id {
                "Master".to_owned()
            } else {
                self.project.tracks[self.automation_mixer_channel_id]
                    .name
                    .clone()
            })
            .show_ui(ui, |ui| {
                for (track_id, lane) in self.project.tracks.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.automation_mixer_channel_id,
                        track_id,
                        &lane.name,
                    );
                }
                ui.selectable_value(&mut self.automation_mixer_channel_id, master_id, "Master");
            });
        let label = match self.automation_mixer_parameter {
            MixerParameter::Send { destination } => {
                format!("Send \u{2192} {}", channel_name(&self.project, destination))
            }
            parameter => parameter.label().to_owned(),
        };
        egui::ComboBox::from_id_salt("automation_mixer_parameter")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for parameter in [
                    MixerParameter::Gain,
                    MixerParameter::Pan,
                    MixerParameter::Width,
                    MixerParameter::LowDb,
                    MixerParameter::MidDb,
                    MixerParameter::HighDb,
                ] {
                    ui.selectable_value(
                        &mut self.automation_mixer_parameter,
                        parameter,
                        parameter.label(),
                    );
                }
                ui.menu_button("Send", |ui| {
                    for destination in 0..=self.project.tracks.len() {
                        if destination == self.automation_mixer_channel_id {
                            continue;
                        }
                        let parameter = MixerParameter::Send { destination };
                        let name = channel_name(&self.project, destination);
                        if ui
                            .selectable_value(
                                &mut self.automation_mixer_parameter,
                                parameter,
                                format!("Send \u{2192} {name}"),
                            )
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
            });
        self.selected_automation_target()
    }

    /// Clip picker: a clip's playback controls or the sample defaults.
    fn pick_clip_target(&mut self, ui: &mut egui::Ui) -> Option<ParameterTarget> {
        ui.checkbox(&mut self.automation_sample_defaults, "All samples")
            .on_hover_text("Automate the defaults every audio clip inherits.");
        if !self.automation_sample_defaults {
            self.automation_clip_lane = self
                .automation_clip_lane
                .min(self.project.tracks.len().saturating_sub(1));
            egui::ComboBox::from_id_salt("automation_clip_lane")
                .selected_text(
                    self.project
                        .tracks
                        .get(self.automation_clip_lane)
                        .map_or("Track", |lane| lane.name.as_str()),
                )
                .show_ui(ui, |ui| {
                    for (lane_id, lane) in self.project.tracks.iter().enumerate() {
                        ui.selectable_value(&mut self.automation_clip_lane, lane_id, &lane.name);
                    }
                });
            let clip_count = self
                .project
                .tracks
                .get(self.automation_clip_lane)
                .map_or(0, |lane| lane.clips.len());
            if clip_count == 0 {
                ui.label("That track has no clips.");
                return None;
            }
            self.automation_clip_id = self.automation_clip_id.min(clip_count - 1);
            egui::ComboBox::from_id_salt("automation_clip_id")
                .selected_text(format!("Clip {}", self.automation_clip_id + 1))
                .show_ui(ui, |ui| {
                    for clip_id in 0..clip_count {
                        ui.selectable_value(
                            &mut self.automation_clip_id,
                            clip_id,
                            format!("Clip {}", clip_id + 1),
                        );
                    }
                });
        }
        egui::ComboBox::from_id_salt("automation_clip_parameter")
            .selected_text(self.automation_clip_parameter.label())
            .show_ui(ui, |ui| {
                for parameter in ClipParameter::ALL {
                    ui.selectable_value(
                        &mut self.automation_clip_parameter,
                        parameter,
                        parameter.label(),
                    );
                }
            });
        self.selected_automation_target()
    }
}
