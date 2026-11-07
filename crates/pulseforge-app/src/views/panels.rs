//! Routing, MIDI, footer and status panels.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn meter_level(&self, channel: usize) -> f32 {
        self.audio
            .as_ref()
            .and_then(|audio| {
                audio
                    .meters
                    .try_lock()
                    .ok()
                    .and_then(|values| values.get(channel).copied())
            })
            .unwrap_or(0.0)
    }

    pub(crate) fn render_routing_cables(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        profile_scope!("render_routing_cables");
        ui.horizontal(|ui| {
            ui.heading("Routing cables");
            ui.label(
                "Drag outputs to inputs · hover a cable for level · right-click to disconnect",
            );
        });
        let track_count = self.project.tracks.len();
        let channel_count = track_count + 1;
        let mut order = vec![track_count];
        let mut lanes: Vec<_> = (0..track_count).collect();
        lanes.sort_by_key(|lane| !self.project.tracks[*lane].pinned);
        order.extend(lanes);
        let canvas_height = 48.0 + channel_count as f32 * 38.0;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(420.0), canvas_height),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 5.0, egui::Color32::from_gray(22));
        let source_x = rect.left() + 156.0;
        let target_x = rect.right() - 156.0;
        painter.text(
            egui::pos2(rect.left() + 18.0, rect.top() + 18.0),
            egui::Align2::LEFT_CENTER,
            "CHANNEL OUTPUTS",
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(155),
        );
        painter.text(
            egui::pos2(rect.right() - 18.0, rect.top() + 18.0),
            egui::Align2::RIGHT_CENTER,
            "CHANNEL INPUTS",
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(155),
        );
        let channel_name = |index: usize, project: &Project| {
            if index == project.tracks.len() {
                "MASTER".to_owned()
            } else {
                project.tracks[index].name.clone()
            }
        };
        let mut node_y = vec![0.0; channel_count];
        for (row, channel) in order.iter().copied().enumerate() {
            node_y[channel] = rect.top() + 48.0 + row as f32 * 38.0;
        }
        let channel_color = |index: usize, project: &Project| {
            if index == project.tracks.len() {
                egui::Color32::WHITE
            } else {
                lane_color(index)
            }
        };
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let mut nearest_route = None;
        let mut nearest_distance = f32::INFINITY;
        let mut routes = Vec::new();
        for source in 0..channel_count {
            for destination in 0..channel_count {
                let gain = self.project.sends[source][destination];
                if gain <= 0.001 {
                    continue;
                }
                let start = egui::pos2(source_x, node_y[source]);
                let end = egui::pos2(target_x, node_y[destination]);
                let control_x = (start.x + end.x) * 0.5;
                let control = egui::pos2(control_x, (start.y + end.y) * 0.5);
                let mut points = Vec::with_capacity(25);
                for segment in 0..=24 {
                    let t = segment as f32 / 24.0;
                    let one_minus_t = 1.0 - t;
                    points.push(egui::pos2(
                        one_minus_t * one_minus_t * start.x
                            + 2.0 * one_minus_t * t * control.x
                            + t * t * end.x,
                        one_minus_t * one_minus_t * start.y
                            + 2.0 * one_minus_t * t * control.y
                            + t * t * end.y,
                    ));
                }
                if let Some(pointer) = pointer.filter(|point| rect.contains(*point)) {
                    let distance = points
                        .windows(2)
                        .map(|pair| point_segment_distance(pointer, pair[0], pair[1]))
                        .fold(f32::INFINITY, f32::min);
                    if distance < nearest_distance {
                        nearest_distance = distance;
                        nearest_route = Some((source, destination, points.clone()));
                    }
                }
                routes.push((source, destination, gain, points));
            }
        }
        let hovered_route = (nearest_distance < 13.0).then_some(nearest_route).flatten();
        for (source, destination, gain, points) in &routes {
            // The cable geometry and the colour blend come from the shared curve
            // type, so this view and any other cable drawing stay in step.
            let mut cable = pulseforge_widgets::shape::ScreenCurve::new();
            for point in points {
                cable.push(*point);
            }
            let width = if hovered_route
                .as_ref()
                .is_some_and(|(from, to, _)| from == source && to == destination)
            {
                3.5
            } else {
                1.2 + *gain * 2.0
            };
            cable.paint_gradient(
                &painter,
                width,
                channel_color(*source, &self.project),
                channel_color(*destination, &self.project),
            );
        }

        for index in order.iter().copied() {
            let y = node_y[index];
            let name = channel_name(index, &self.project);
            let color = channel_color(index, &self.project);
            painter.text(
                egui::pos2(rect.left() + 18.0, y),
                egui::Align2::LEFT_CENTER,
                &name,
                egui::FontId::proportional(12.0),
                color,
            );
            painter.text(
                egui::pos2(rect.right() - 18.0, y),
                egui::Align2::RIGHT_CENTER,
                &name,
                egui::FontId::proportional(12.0),
                color,
            );
            painter.circle_filled(egui::pos2(source_x, y), 6.0, color);
            painter.circle_filled(egui::pos2(target_x, y), 6.0, color);
        }

        let (pressed, secondary_pressed, released, secondary_released) = ui.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.pointer.button_released(egui::PointerButton::Primary),
                input
                    .pointer
                    .button_released(egui::PointerButton::Secondary),
            )
        });
        let mut changed = false;
        if secondary_pressed && let Some(pointer) = pointer.filter(|point| rect.contains(*point)) {
            let node = order.iter().copied().find(|index| {
                pointer.distance(egui::pos2(source_x, node_y[*index])) <= 12.0
                    || pointer.distance(egui::pos2(target_x, node_y[*index])) <= 12.0
            });
            if let Some(channel) = node {
                disconnect_channel(&mut self.project.sends, channel);
                changed = true;
            } else if let Some((source, destination, _)) = &hovered_route {
                self.project.sends[*source][*destination] = 0.0;
                changed = true;
            }
        }
        if pressed && let Some(pointer) = pointer.filter(|point| rect.contains(*point)) {
            self.routing_drag_from = order
                .iter()
                .copied()
                .find(|index| pointer.distance(egui::pos2(source_x, node_y[*index])) <= 12.0);
        }
        if let Some(source) = self.routing_drag_from
            && let Some(pointer) = pointer.filter(|point| rect.contains(*point))
        {
            let end = egui::pos2(pointer.x, pointer.y);
            painter.line_segment(
                [egui::pos2(source_x, node_y[source]), end],
                egui::Stroke::new(2.0_f32, channel_color(source, &self.project)),
            );
            if released {
                if let Some(destination) = order
                    .iter()
                    .copied()
                    .find(|index| pointer.distance(egui::pos2(target_x, node_y[*index])) <= 12.0)
                {
                    if self.project.sends[source][destination] <= 0.001 {
                        self.project.sends[source][destination] = 0.25;
                        changed = true;
                    }
                }
            }
        }
        if released || secondary_released {
            self.routing_drag_from = None;
        }

        if let Some((source, destination)) = hovered_route
            .map(|(source, destination, _)| (source, destination))
            .or(self.routing_gain_route)
        {
            let middle = {
                let start = egui::pos2(source_x, node_y[source]);
                let end = egui::pos2(target_x, node_y[destination]);
                let control = egui::pos2((start.x + end.x) * 0.5, (start.y + end.y) * 0.5);
                // Midpoint of the quadratic curve at t = 0.5.
                egui::pos2(
                    0.25 * start.x + 0.5 * control.x + 0.25 * end.x,
                    0.25 * start.y + 0.5 * control.y + 0.25 * end.y,
                )
            };
            let knob_rect = egui::Rect::from_center_size(middle, egui::vec2(34.0, 34.0));
            // The send level uses the shared knob widget, so it drags, resets and
            // offers its context menu exactly like every other parameter.
            let mut value = self.project.sends[source][destination];
            let send_target = ParameterTarget::Track { track_id: source, parameter: TrackParameter::Mixer(project::MixerParameter::Send { destination }) };
            ui.scope_builder(egui::UiBuilder::new().max_rect(knob_rect), |ui| {
                if Knob::new("", 0.0, 0.95, "")
                    .with_default(0.25)
                    .compact()
                    .automates(send_target)
                    .show(ui, &mut value)
                {
                    self.project.sends[source][destination] = value;
                    changed = true;
                }
            });
        }

        for channel in order.iter().copied() {
            let node_rect = egui::Rect::from_center_size(
                egui::pos2(source_x, node_y[channel]),
                egui::vec2(24.0, 24.0),
            );
            let node_response = ui.interact(
                node_rect,
                egui::Id::new(("route_output", channel)),
                egui::Sense::click(),
            );
            node_response.on_hover_text("Drag to a channel input to create a send");
        }
        let _response = response;
        ui.label("Active sends");
        for (source, destination, _, _) in &routes {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{}  ->  {}",
                    channel_name(*source, &self.project),
                    channel_name(*destination, &self.project)
                ));
                let mut gain = self.project.sends[*source][*destination];
                let send_target = ParameterTarget::MixerChannel {
                    track_id: *source,
                    parameter: project::MixerParameter::Send {
                        destination: *destination,
                    },
                };
                let response = ui.add(
                    egui::Slider::new(&mut gain, 0.0..=0.95)
                        .show_value(true)
                        .text("Level"),
                );
                automation_context_menu(&response, &format!("Send · {gain:.2}"), send_target);
                if response.changed() {
                    self.project.sends[*source][*destination] = gain;
                    changed = true;
                }
                if ui.small_button("Remove").clicked() {
                    self.project.sends[*source][*destination] = 0.0;
                    changed = true;
                }
            });
        }
        if changed {
            self.project.normalize();
            self.sync_project();
        }
        context.request_repaint_after(std::time::Duration::from_millis(40));
    }

    pub(crate) fn render_midi(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Refresh inputs").clicked() {
                match Self::query_midi_ports() {
                    Ok(ports) => {
                        self.midi_ports = ports;
                        self.midi_discovery_error = None;
                    }
                    Err(error) => self.midi_discovery_error = Some(error),
                }
            }
            let selected_name = self
                .selected_midi_port
                .and_then(|id| self.midi_ports.get(id))
                .map_or("Select input", String::as_str);
            egui::ComboBox::from_id_salt("midi_input")
                .selected_text(selected_name)
                .show_ui(ui, |ui| {
                    for (id, name) in self.midi_ports.clone().iter().enumerate() {
                        if ui
                            .selectable_label(self.selected_midi_port == Some(id), name)
                            .clicked()
                        {
                            self.connect_midi(id);
                        }
                    }
                });
            if self.midi_connection.is_some() && ui.button("Disconnect").clicked() {
                self.midi_connection.take();
                self.selected_midi_port = None;
            }
        });
        if let Some(error) = &self.midi_discovery_error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        ui.separator();
        egui::Grid::new("midi_route_grid")
            .striped(true)
            .show(ui, |ui| {
                ui.label("Input channel");
                ui.label("Instrument");
                ui.end_row();
                for channel in 0..MIDI_CHANNEL_COUNT {
                    ui.label(format!("Channel {}", channel + 1));
                    let mut instrument = self.project.midi_routes[channel];
                    egui::ComboBox::from_id_salt(("route", channel))
                        .selected_text(
                            self.project
                                .tracks
                                .get(instrument)
                                .map_or("Instrument", |item| item.name.as_str())
                                .to_owned(),
                        )
                        .show_ui(ui, |ui| {
                            for (id, item) in self.project.tracks.iter().enumerate() {
                                ui.selectable_value(&mut instrument, id, &item.name);
                            }
                        });
                    if instrument != self.project.midi_routes[channel] {
                        self.project.midi_routes[channel] = instrument;
                        self.sync_project();
                    }
                    ui.end_row();
                }
            });
    }

    pub(crate) fn render_footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.project_path)
                    .desired_width(230.0)
                    .hint_text("session.json"),
            );
            if ui.button("Save").clicked() {
                self.save_project();
            }
            if ui.button("Load").clicked() {
                self.load_project();
            }
            ui.separator();
            ui.label(format!("Selected: {}", self.selection_label()));
            ui.separator();
            if let Some(audio) = &self.audio {
                ui.label(format!("{} Hz", audio.sample_rate));
            } else if let Some(error) = &self.audio_error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            // Background jobs are visible, so a slow scan never looks like a
            // hang.
            let running = self.tasks.running();
            if running > 0 {
                // Live machine/engine numbers while the debug panel is open.
                if !self.debug_summary.is_empty() {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(self.debug_summary.clone())
                            .small()
                            .weak(),
                    );
                }
                ui.label(format!("{running} background job(s)"));
            }
            ui.separator();
            // Hovered control details: name, value and unit (tooltip replacement).
            let hovered = ui.data(|data| {
                data.get_temp::<String>(egui::Id::new(HOVER_TEXT_KEY))
                    .filter(|text| !text.is_empty())
            });
            if let Some(text) = hovered {
                ui.label(
                    egui::RichText::new(text)
                        .color(self.ui_accent())
                        .monospace(),
                );
                ui.separator();
            }
            let live = self.status.clone();
            ui.label(live);
        });
    }

    pub(crate) fn selection_label(&self) -> String {
        match self.selection {
            Some(Selection::Note {
                pattern,
                instrument,
                pitch,
                start_step,
            }) => self
                .project
                .tracks
                .get(pattern)
                .and_then(|pattern| {
                    pattern.notes.iter().find(|note| {
                        note.instrument_id == instrument
                            && note.pitch == pitch
                            && note.start_step == start_step
                    })
                })
                .map_or_else(
                    || "note".to_owned(),
                    |note| format!("note {} @ {}", note_name(note.pitch), note.start_step),
                ),
            Some(Selection::Instrument(instrument)) => self
                .project
                .tracks
                .get(instrument)
                .map_or_else(|| "instrument".to_owned(), |item| item.name.clone()),
            Some(Selection::Clip { track: lane, clip }) => self
                .project
                .tracks
                .get(lane)
                .and_then(|lane| lane.clips.get(clip))
                .map_or_else(
                    || "clip".to_owned(),
                    |item| match &item.source {
                        ClipSource::Pattern { pattern_id } => {
                            self.project.tracks.get(*pattern_id).map_or_else(
                                || "pattern clip".to_owned(),
                                |pattern| pattern.name.clone(),
                            )
                        }
                        ClipSource::Audio { path } => std::path::Path::new(path)
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .unwrap_or("audio clip")
                            .to_owned(),
                    },
                ),
            Some(Selection::MixerChannel(channel)) => self
                .project
                .tracks
                .get(channel)
                .map_or_else(|| "MASTER".to_owned(), |lane| lane.name.clone()),
            Some(Selection::Automation(track)) => self
                .project
                .automation_tracks
                .get(track)
                .map_or_else(|| "automation".to_owned(), |track| track.name.clone()),
            None => "none".to_owned(),
        }
    }
}
