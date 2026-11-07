//! The channel rack: one row per source track, with its step lane.

use crate::*;

impl PulseForgeApp {
    /// Draws the channel rack and its step lanes.
    ///
    /// # Parameters
    ///
    /// - `ui`: parent UI.
    /// - `context`: egui context, used for pointer input.
    pub(crate) fn render_channel_rack(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        profile_scope!("render_channel_rack");
        ui.horizontal(|ui| {
            ui.label("Track");
            let track_name = self
                .project
                .tracks
                .get(self.selected_instrument)
                .map_or("—", |track| track.name.as_str())
                .to_owned();
            egui::ComboBox::from_id_salt("rack_track")
                .selected_text(track_name)
                .show_ui(ui, |ui| {
                    for (id, track) in self.project.tracks.iter().enumerate() {
                        if track.is_group() {
                            continue;
                        }
                        ui.selectable_value(&mut self.selected_instrument, id, &track.name);
                    }
                });
            ui.label("Clip");
            let clip_names: Vec<String> = self
                .project
                .tracks
                .get(self.selected_instrument)
                .map(|track| {
                    track
                        .clips
                        .iter()
                        .enumerate()
                        .map(|(index, clip)| format!("{}: {}", index + 1, clip.name))
                        .collect()
                })
                .unwrap_or_default();
            let selected_clip_name = clip_names
                .get(self.selected_clip)
                .cloned()
                .unwrap_or_else(|| "—".to_owned());
            egui::ComboBox::from_id_salt("rack_clip")
                .selected_text(selected_clip_name)
                .show_ui(ui, |ui| {
                    for (index, name) in clip_names.iter().enumerate() {
                        ui.selectable_value(&mut self.selected_clip, index, name);
                    }
                });
            ui.label("Length (steps)");
            let mut length = self.active_clip().map_or(16, |clip| clip.length_steps);
            if ui
                .add(
                    egui::DragValue::new(&mut length)
                        .range(1..=u64::MAX)
                        .speed(1.0),
                )
                .changed()
            {
                if let Some(clip) = self.active_clip_mut() {
                    clip.length_steps = length.max(1);
                }
                self.sync_project();
            }
            if ui.button("+ Track").clicked() {
                let id = self.project.add_track();
                self.selected_instrument = id;
                self.selected_clip = 0;
                self.selection = Some(Selection::Instrument(id));
                self.sync_project();
            }
            ui.label(format!(
                "Page {} · Middle-drag to pan",
                self.pattern_pan_steps / VISIBLE_PATTERN_STEPS + 1
            ));
        });
        ui.separator();

        let start = self.pattern_pan_steps;
        let end = start.saturating_add(VISIBLE_PATTERN_STEPS);
        let track_count = self.project.tracks.len();
        for track_id in 0..track_count {
            if self.project.tracks[track_id].is_group() {
                continue;
            }
            ui.horizontal(|ui| {
                // Name and plug-in button are one control: clicking toggles the
                // plug-in window, right-clicking opens the track menu.
                let track_color = self.instrument_color(track_id);
                let (enabled, locked, solo) = {
                    let track = &self.project.tracks[track_id];
                    (track.enabled, track.locked, track.solo)
                };
                let is_open = self.open_instrument_plugins.contains(&track_id);
                let rename_here = self.renaming_instrument == Some(track_id);
                if rename_here {
                    let buffer = &mut self.rename_buffer;
                    let response = ui.add_sized(
                        [110.0, 25.0],
                        egui::TextEdit::singleline(buffer).hint_text("Name"),
                    );
                    response.request_focus();
                    let (enter, escape) = ui.input(|input| {
                        (
                            input.key_pressed(egui::Key::Enter),
                            input.key_pressed(egui::Key::Escape),
                        )
                    });
                    if enter {
                        self.commit_instrument_rename();
                    } else if escape {
                        self.renaming_instrument = None;
                    }
                } else {
                    let label =
                        egui::RichText::new(&self.project.tracks[track_id].name).color(track_color);
                    let response = ui.add_sized(
                        [110.0, 25.0],
                        egui::Button::new(label)
                            .fill(if is_open {
                                track_color.gamma_multiply(0.35)
                            } else {
                                egui::Color32::from_gray(32)
                            })
                            .sense(egui::Sense::click_and_drag()),
                    );
                    if response.clicked() {
                        self.selected_instrument = track_id;
                        self.selection = Some(Selection::Instrument(track_id));
                        if is_open {
                            self.open_instrument_plugins.remove(&track_id);
                        } else {
                            self.open_instrument_plugins.insert(track_id);
                        }
                    }
                    if response.drag_started() {
                        self.pattern_drag = None;
                    }
                    let channel_menu = ContextMenu {
                        items: vec![
                            MenuItem::Action {
                                label: "Rename".to_owned(),
                                action: ContextAction::RenameInstrument(track_id),
                            },
                            MenuItem::Action {
                                label: "Remove".to_owned(),
                                action: ContextAction::RemoveInstrument(track_id),
                            },
                            MenuItem::Action {
                                label: "Group in a folder".to_owned(),
                                action: ContextAction::GroupTrack(track_id),
                            },
                            MenuItem::Folder {
                                label: "Colour".to_owned(),
                                items: vec![
                                    MenuItem::Action {
                                        label: "Default".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: track_id,
                                            color: None,
                                        },
                                    },
                                    MenuItem::Action {
                                        label: "Teal".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: track_id,
                                            color: Some([112, 208, 190]),
                                        },
                                    },
                                    MenuItem::Action {
                                        label: "Amber".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: track_id,
                                            color: Some([230, 178, 92]),
                                        },
                                    },
                                    MenuItem::Action {
                                        label: "Rose".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: track_id,
                                            color: Some([226, 120, 140]),
                                        },
                                    },
                                    MenuItem::Action {
                                        label: "Violet".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: track_id,
                                            color: Some([160, 140, 230]),
                                        },
                                    },
                                ],
                            },
                            MenuItem::Separator,
                            MenuItem::Action {
                                label: "Choose plug-in?".to_owned(),
                                action: ContextAction::OpenInstrumentPicker(track_id),
                            },
                        ],
                    };
                    let mut channel_action = None;
                    response.context_menu(|ui| channel_action = channel_menu.show(ui));
                    if let Some(action) = channel_action {
                        self.apply_context_action(action);
                    }
                }
                // One power switch; shift locks, ctrl solos.
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(18.0, 20.0), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    3.0,
                    if enabled {
                        track_color.gamma_multiply(0.45)
                    } else {
                        egui::Color32::from_gray(30)
                    },
                );
                paint_icon(
                    ui.painter(),
                    rect.shrink(3.0),
                    Icon::Power,
                    if enabled {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::from_gray(110)
                    },
                );
                if locked {
                    ui.painter().line_segment(
                        [
                            egui::pos2(rect.left() + 2.0, rect.bottom() - 2.0),
                            egui::pos2(rect.right() - 2.0, rect.bottom() - 2.0),
                        ],
                        egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(230, 178, 92)),
                    );
                }
                if solo {
                    ui.painter().circle_filled(
                        egui::pos2(rect.right() - 3.0, rect.top() + 3.0),
                        2.5,
                        egui::Color32::from_rgb(120, 200, 255),
                    );
                }
                tooltip(ui, &response, "Power - shift locks, ctrl solos");
                if response.clicked() {
                    self.toggle_instrument_switch(track_id);
                }
                let (mut gain, mut pan) = self
                    .project
                    .tracks
                    .get(track_id)
                    .and_then(|track| track.generator())
                    .map_or((0.8, 0.0), |generator| (generator.gain, generator.pan));
                let gain_target = ParameterTarget::Track {
                    track_id,
                    parameter: TrackParameter::Generator(InstrumentParameter::Gain),
                };
                let pan_target = ParameterTarget::Track {
                    track_id,
                    parameter: TrackParameter::Generator(InstrumentParameter::Pan),
                };
                let gain_changed = Knob::new("VOL", 0.0, 1.0, "")
                    .compact()
                    .with_default(0.8)
                    .automates(gain_target)
                    .show(ui, &mut gain);
                let pan_changed = Knob::new("PAN", -1.0, 1.0, "")
                    .compact()
                    .with_default(0.0)
                    .automates(pan_target)
                    .show(ui, &mut pan);
                if gain_changed || pan_changed {
                    if let Some(generator) = self
                        .project
                        .tracks
                        .get_mut(track_id)
                        .and_then(|track| track.generator_mut())
                    {
                        generator.gain = gain;
                        generator.pan = pan;
                    }
                    self.sync_project();
                }
                let clip_index = self.resolve_active_clip().unwrap_or(0);
                let clip_length = self.active_clip().map_or(16, |clip| clip.length_steps);
                // A fixed column keeps every row aligned; the rack window's scroll
                // area handles overflow instead of squeezing the steps.
                let step_width = STEP_COLUMN_WIDTH;
                let grid_width = step_width * VISIBLE_PATTERN_STEPS as f32;
                let (grid_rect, grid_response) = ui.allocate_exact_size(
                    egui::vec2(grid_width, 26.0),
                    egui::Sense::click_and_drag(),
                );
                let painter = ui.painter_at(grid_rect);
                for step in start..end {
                    let active = self
                        .project
                        .tracks
                        .get(track_id)
                        .is_some_and(|track| step_is_active(track, clip_index, step));
                    let color = if step >= clip_length {
                        egui::Color32::from_gray(19)
                    } else if active {
                        lane_color(track_id)
                    } else if step % 4 == 0 {
                        egui::Color32::from_gray(62)
                    } else {
                        egui::Color32::from_gray(38)
                    };
                    let step_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            grid_rect.left() + (step - start) as f32 * step_width,
                            grid_rect.top(),
                        ),
                        egui::vec2(step_width, 26.0),
                    );
                    painter.rect_filled(step_rect.shrink(1.0), 3.0, color);
                    let marker_color = if active {
                        egui::Color32::from_gray(245)
                    } else {
                        egui::Color32::from_gray(105)
                    };
                    painter.circle_filled(
                        step_rect.center(),
                        if active { 3.0 } else { 1.5 },
                        marker_color,
                    );
                }
                let (primary_pressed, secondary_pressed, primary_down, secondary_down) = context
                    .input(|input| {
                        (
                            input.pointer.button_pressed(egui::PointerButton::Primary),
                            input.pointer.button_pressed(egui::PointerButton::Secondary),
                            input.pointer.button_down(egui::PointerButton::Primary),
                            input.pointer.button_down(egui::PointerButton::Secondary),
                        )
                    });
                let pointer = context.input(|input| input.pointer.interact_pos());
                if let Some(position) = pointer.filter(|position| {
                    grid_rect.contains(*position)
                        && (grid_response.contains_pointer()
                            || self.rack_gesture.is_some()
                            || self.pattern_drag.is_some())
                }) {
                    let step = rack_step_at(
                        position.x,
                        grid_rect.left(),
                        start,
                        step_width,
                        VISIBLE_PATTERN_STEPS,
                    );
                    if primary_pressed || secondary_pressed {
                        let enabled = primary_pressed;
                        self.selected_instrument = track_id;
                        self.selected_clip = clip_index;
                        self.selection = Some(Selection::Instrument(track_id));
                        self.set_step_hit(track_id, clip_index, step, enabled);
                        self.rack_gesture = Some(RackGesture {
                            track_id,
                            clip_id: clip_index,
                            enabled,
                            last_step: step,
                        });
                    } else if (primary_down || secondary_down)
                        && grid_response.hovered()
                        && let Some(mut gesture) = self.rack_gesture
                        && gesture.track_id == track_id
                        && gesture.clip_id == clip_index
                        && gesture.last_step != step
                    {
                        let (first_step, last_step) = rack_step_span(gesture.last_step, step);
                        for painted_step in first_step..=last_step {
                            self.set_step_hit(track_id, clip_index, painted_step, gesture.enabled);
                        }
                        gesture.last_step = step;
                        self.rack_gesture = Some(gesture);
                    }
                }
                if ui.small_button("Piano").clicked() {
                    self.selected_instrument = track_id;
                    self.selected_clip = clip_index;
                    self.selection = Some(Selection::Instrument(track_id));
                    self.windows.piano_roll = true;
                }
            });
        }
        let drag_finished = context.input(|input| {
            !input.pointer.button_down(egui::PointerButton::Primary)
                && !input.pointer.button_down(egui::PointerButton::Secondary)
        });
        if drag_finished && self.rack_gesture.take().is_some() {
            self.sync_project();
        }

        let rect = ui.available_rect_before_wrap();
        context.input(|input| {
            if input.pointer.button_down(egui::PointerButton::Middle)
                && rect.contains(input.pointer.interact_pos().unwrap_or_default())
                && input.pointer.delta().x != 0.0
            {
                self.pattern_pan_steps = self
                    .pattern_pan_steps
                    .saturating_add_signed((-input.pointer.delta().x / 22.0).round() as i64);
            }
            // The wheel scrolls across steps as well, which is what the rack was
            // missing: only middle-drag used to pan.
            let scroll = input.raw_scroll_delta.y;
            if scroll.abs() > 0.01
                && rect.contains(input.pointer.interact_pos().unwrap_or_default())
            {
                let direction = if scroll > 0.0 { -2 } else { 2 };
                self.pattern_pan_steps = self.pattern_pan_steps.saturating_add_signed(direction);
            }
        });
        ui.label(
            "Left-drag paints steps on; right-drag paints off. Each row writes into the selected clip of its track.",
        );
    }

    /// Toggles one step of a track's clip.
    ///
    /// # Parameters
    ///
    /// - `track_id`: the track whose clip is edited.
    /// - `clip_index`: clip slot on that track.
    /// - `step`: step inside the clip.
    /// - `enabled`: `true` to add the hit, `false` to remove it.
    ///
    /// # Returns
    ///
    /// `true` when the clip changed.
    pub(crate) fn set_step_hit(
        &mut self,
        track_id: usize,
        clip_index: usize,
        step: u64,
        enabled: bool,
    ) -> bool {
        let velocity = self.settings.default_velocity.clamp(1, 127);
        let Some(clip) = self
            .project
            .tracks
            .get_mut(track_id)
            .and_then(|track| track.clips.get_mut(clip_index))
        else {
            return false;
        };
        if enabled && step >= clip.length_steps {
            // Activating a step outside the range grows the clip, so the range
            // always covers what the user painted.
            clip.length_steps = step + 1;
        }
        let existing = clip.notes.iter().position(|note| {
            note.pitch == project::STEP_SEQUENCER_PITCH && note.start_step == step
        });
        match (enabled, existing) {
            (true, None) => {
                let mut note = ClipNote::new(step, project::STEP_SEQUENCER_PITCH, velocity, 1);
                note.set_start_tick(step.saturating_mul(16));
                note.set_duration_ticks(16);
                clip.notes.push(note);
                true
            }
            (false, Some(index)) => {
                clip.notes.remove(index);
                true
            }
            _ => false,
        }
    }
}

/// True when a track's clip holds a step-sequencer note at `step`.
///
/// # Parameters
///
/// - `track`: the track to inspect.
/// - `clip_index`: clip slot on that track.
/// - `step`: step to test.
fn step_is_active(track: &Track, clip_index: usize, step: u64) -> bool {
    track.clips.get(clip_index).is_some_and(|clip| {
        clip.notes
            .iter()
            .any(|note| note.pitch == project::STEP_SEQUENCER_PITCH && note.start_step == step)
    })
}
