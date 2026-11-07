//! Context-menu and clip actions.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn apply_context_action(&mut self, action: ContextAction) {
        match action {
            ContextAction::ToggleLanePin(lane) => {
                if let Some(lane) = self.project.tracks.get_mut(lane) {
                    lane.pinned = !lane.pinned;
                }
            }
            ContextAction::DisconnectChannel(channel) => {
                disconnect_channel(&mut self.project.sends, channel);
            }
            ContextAction::SelectChannel(channel) => {
                if channel <= self.project.tracks.len() {
                    self.selected_mixer_channel = channel;
                    self.selection = Some(Selection::MixerChannel(channel));
                    if channel < self.project.tracks.len() {
                        self.selected_lane = channel;
                    }
                }
            }
            ContextAction::DeleteClip { lane, clip } => {
                if let Some(lane) = self.project.tracks.get_mut(lane)
                    && clip < lane.clips.len()
                {
                    lane.clips.remove(clip);
                    self.selection = None;
                }
            }
            ContextAction::DuplicateClip { lane, clip } => {
                self.duplicate_clip(lane, clip);
                return;
            }
            ContextAction::MakeClipPatternUnique { lane, clip } => {
                self.make_clip_pattern_unique(lane, clip);
                return;
            }
            ContextAction::RenameTrack(lane) => {
                self.selected_lane = lane;
                self.begin_rename_track();
                return;
            }
            ContextAction::MoveTrack { lane, up } => {
                let target = if up {
                    lane.checked_sub(1)
                } else {
                    Some(lane + 1).filter(|target| *target < self.project.tracks.len())
                };
                if let Some(target) = target {
                    self.push_history();
                    self.project.move_track(lane, target);
                    self.selected_lane = target;
                    self.selection = Some(Selection::MixerChannel(target));
                }
            }
            ContextAction::ToggleTrackAutomation(lane) => {
                if let Some(track) = self.project.tracks.get_mut(lane) {
                    track.automation_collapsed = !track.automation_collapsed;
                }
            }
            ContextAction::DeleteAutomation(track_id) => {
                if track_id < self.project.automation_tracks.len() {
                    self.project.automation_tracks.remove(track_id);
                    self.selection = None;
                }
            }
            ContextAction::DuplicateAutomation(track_id) => {
                if let Some(track) = self.project.automation_tracks.get(track_id).cloned() {
                    // Shift the copy after the original so both clips are
                    // visible side by side on the same parameter.
                    let mut copy = track;
                    copy.start_step = copy.start_step.saturating_add(copy.length_steps.max(1));
                    copy.name = format!("{} copy", copy.name);
                    self.push_history();
                    self.project.automation_tracks.push(copy);
                }
            }
            ContextAction::NewAutomationForTrack(lane) => {
                let target = ParameterTarget::InsertEffect {
                    track_id: lane,
                    slot_id: 0,
                    parameter: EffectParameter::Mix,
                };
                self.push_history();
                self.project.automation_tracks.push(AutomationTrack {
                    name: format!(
                        "{} ? Insert 1 mix",
                        self.project
                            .tracks
                            .get(lane)
                            .map_or("Track", |track| track.name.as_str())
                    ),
                    target,
                    start_step: self.arrangement_pan_steps,
                    length_steps: 32,
                    points: vec![
                        AutomationPoint {
                            step: 0,
                            value: 0.2,
                            curve: AutomationCurve::Linear,
                        },
                        AutomationPoint {
                            step: 32,
                            value: 0.8,
                            curve: AutomationCurve::Smooth,
                        },
                    ],
                    modulation_points: Vec::new(),
                    enabled: true,
                    source: project::AutomationSource::Curve,
                });
            }
            ContextAction::RenameInstrument(instrument) => {
                self.renaming_instrument = Some(instrument);
                self.rename_buffer = self
                    .project
                    .tracks
                    .get(instrument)
                    .map_or(String::new(), |item| item.name.clone());
                return;
            }
            ContextAction::RemoveInstrument(instrument) => {
                if self.project.tracks.len() <= 1 || instrument >= self.project.tracks.len() {
                    self.status = "A project needs at least one track.".to_owned();
                } else {
                    self.push_history();
                    let host_id = pulseforge_core::hosted::instrument_host_id(instrument);
                    self.unload_host(host_id);
                    // Host ids are track indices, so every id above the removed
                    // one shifts down.
                    let mut hosted = std::mem::take(&mut self.hosted);
                    let mut meta = std::mem::take(&mut self.hosted_meta);
                    fn renumber<T>(map: &mut std::collections::HashMap<u64, T>, removed: u64) {
                        let moved: Vec<(u64, T)> = map
                            .drain()
                            .filter_map(|(id, value)| {
                                (id != removed)
                                    .then_some((if id > removed { id - 1 } else { id }, value))
                            })
                            .collect();
                        for (id, value) in moved {
                            map.insert(id, value);
                        }
                    }
                    renumber(&mut hosted, host_id);
                    renumber(&mut meta, host_id);
                    self.hosted = hosted;
                    self.hosted_meta = meta;
                    self.attached_editor = None;
                    // The project handles sends, parents and automation itself.
                    self.project.remove_track(instrument);
                    self.open_instrument_plugins = std::mem::take(&mut self.open_instrument_plugins)
                        .into_iter()
                        .filter_map(|id| match id.cmp(&instrument) {
                            std::cmp::Ordering::Less => Some(id),
                            std::cmp::Ordering::Equal => None,
                            std::cmp::Ordering::Greater => Some(id - 1),
                        })
                        .collect();
                    self.selected_instrument = self
                        .selected_instrument
                        .min(self.project.tracks.len().saturating_sub(1));
                    self.selected_clip = 0;
                    self.selection = None;
                    let last = self.project.tracks.len().saturating_sub(1);
                    for route in &mut self.project.midi_routes {
                        *route = (*route).min(last);
                    }
                    if let Ok(mut routes) = self.midi_routes.lock() {
                        for route in routes.iter_mut() {
                            *route = (*route).min(last);
                        }
                    }
                }
            }            ContextAction::GroupTrack(track) => {
                if self.project.group_track(track).is_some() {
                    self.status = "Grouped the track in a new folder.".to_owned();
                } else {
                    self.status = "That track no longer exists.".to_owned();
                }
            }
            ContextAction::SetInstrumentColor { instrument, color } => {
                if let Some(item) = self.project.tracks.get_mut(instrument) {
                    item.color = color;
                }
            }
            ContextAction::OpenInstrumentPicker(instrument) => {
                self.plugin_selector_target = Some(PluginTarget::Instrument(instrument));
                self.plugin_search.clear();
                return;
            }
            ContextAction::ToggleChannelEnabled(channel) => {
                let solo = self.mod_ctrl;
                let lock = self.mod_shift;
                if channel < self.project.tracks.len() {
                    let lane = &mut self.project.tracks[channel];
                    if lock {
                        lane.locked = !lane.locked;
                    } else if solo {
                        lane.solo = !lane.solo;
                    } else {
                        lane.enabled = !lane.enabled;
                    }
                }
            }
            ContextAction::ToggleInsertEnabled {
                channel,
                slot,
                intent,
            } => {
                if let Some(effect) = self.effect_slot_mut(channel, slot) {
                    if intent.lock {
                        effect.locked = !effect.locked;
                    } else if intent.solo {
                        effect.solo = !effect.solo;
                    } else if intent.toggle {
                        effect.enabled = !effect.enabled;
                    }
                }
            }
            ContextAction::OpenInsertPicker { channel, slot } => {
                self.plugin_selector_target = Some(PluginTarget::Effect { channel, slot });
                self.plugin_search.clear();
                return;
            }
            ContextAction::OpenInsertWindow { channel, slot } => {
                let has_slot = self
                    .effect_slot_name(channel, slot)
                    .is_some_and(|name| !name.is_empty());
                if has_slot {
                    self.open_effect_plugins.insert((channel, slot));
                } else {
                    self.plugin_selector_target = Some(PluginTarget::Effect { channel, slot });
                }
                return;
            }
            ContextAction::ClearInsert { channel, slot } => {
                if let Some(effect) = self.effect_slot_mut(channel, slot) {
                    effect.name.clear();
                    effect.plugin_path.clear();
                    effect.enabled = false;
                    effect.solo = false;
                }
                let host_id = pulseforge_core::hosted::effect_host_id(channel, slot);
                self.unload_host(host_id);
                self.open_effect_plugins.remove(&(channel, slot));
            }
            ContextAction::OpenPluginSettings => {
                self.windows.settings = true;
                self.settings_tab = SettingsTab::Plugins;
                return;
            }
        }
        self.sync_project();
    }

    pub(crate) fn duplicate_selected_clip(&mut self) {
        if let Some(Selection::Clip { track: lane, clip }) = self.selection {
            self.duplicate_clip(lane, clip);
        } else {
            self.status = "Select an arrangement clip to duplicate.".to_owned();
        }
    }

    pub(crate) fn delete_selected_clip(&mut self) {
        if let Some(Selection::Clip { track: lane, clip }) = self.selection
            && let Some(lane) = self.project.tracks.get_mut(lane)
            && clip < lane.clips.len()
        {
            lane.clips.remove(clip);
            self.selection = None;
            self.sync_project();
        }
    }

    pub(crate) fn duplicate_clip(&mut self, lane: usize, clip: usize) {
        let audio_path = self
            .project
            .tracks
            .get(lane)
            .and_then(|lane| lane.clips.get(clip))
            .and_then(|clip| clip.audio_path.clone());
        let Some(duplicate_id) = self.project.duplicate_clip_after(lane, clip) else {
            self.status = "That arrangement clip is no longer available.".to_owned();
            return;
        };
        self.selected_lane = lane;
        self.selection = Some(Selection::Clip {
            track: lane,
            clip: duplicate_id,
        });
        self.sync_project();

        if let Some(path) = audio_path {
            match audio::read_wav(&path) {
                Ok(sample) => {
                    let frame_count = sample.frames.len();
                    if self.send_audio(AudioCommand::LoadAudioClip {
                        path: path.clone(),
                        sample,
                    }) {
                        self.status = format!("Duplicated audio clip ({frame_count} frames).");
                    }
                }
                Err(error) => {
                    self.status = format!("Clip duplicated but WAV reload failed: {error}");
                }
            }
        } else {
            self.status = "Duplicated the clip after its original.".to_owned();
        }
    }

    /// Selects a clip and notes that it already owns its notes.
    ///
    /// # Parameters
    ///
    /// - `track`: track owning the clip.
    /// - `clip`: clip index inside the track.
    pub(crate) fn make_clip_pattern_unique(&mut self, track: usize, clip: usize) {
        self.selected_lane = track;
        self.selected_clip = clip;
        self.selection = Some(Selection::Clip { track, clip });
        self.sync_project();
        self.status = "Clips own their notes, so they are always unique.".to_owned();
    }
}
