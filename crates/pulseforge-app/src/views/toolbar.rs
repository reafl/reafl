//! The toolbar and the clip picker beside the arrangement.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        profile_scope!("render_toolbar");
        // Two compact icon rows: window/transport on top, tempo/grid below.
        // Every control uses the same row height so the bar stays aligned.
        const ROW_HEIGHT: f32 = 24.0;
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.spacing_mut().interact_size.y = ROW_HEIGHT;
        ui.horizontal(|ui| {
            ui.set_height(ROW_HEIGHT);
            ui.label(
                egui::RichText::new("PULSEFORGE")
                    .color(egui::Color32::from_rgb(112, 208, 190))
                    .strong(),
            );
            ui.separator();
            for (icon, hint, action) in [
                (
                    Icon::Rack,
                    "Channel rack (F6)",
                    ShortcutAction::WindowChannelRack,
                ),
                (
                    Icon::Roll,
                    "Piano roll (F7)",
                    ShortcutAction::WindowPianoRoll,
                ),
                (Icon::Mixer, "Mixer (F9)", ShortcutAction::WindowMixer),
                (
                    Icon::Routing,
                    "Routing (Shift+F9)",
                    ShortcutAction::WindowRouting,
                ),
                (
                    Icon::Automation,
                    "Automation (F10)",
                    ShortcutAction::WindowAutomation,
                ),
                (
                    Icon::Samples,
                    "Samples (F11)",
                    ShortcutAction::WindowSamples,
                ),
                (Icon::Midi, "MIDI routing (F8)", ShortcutAction::WindowMidi),
                (Icon::Settings, "Settings", ShortcutAction::WindowSettings),
                (Icon::Render, "Render WAV", ShortcutAction::WindowRender),
                (
                    Icon::Debug,
                    "Debug / profiler (Ctrl+Shift+D)",
                    ShortcutAction::WindowDebug,
                ),
            ] {
                if icon_button(ui, icon, hint).clicked() {
                    self.perform(action);
                }
            }
            ui.separator();
            if icon_button(ui, Icon::Undo, "Undo (Ctrl+Z)").clicked() {
                self.undo();
            }
            if icon_button(ui, Icon::Redo, "Redo (Ctrl+Shift+Z)").clicked() {
                self.redo();
            }
            ui.separator();
            if icon_button(
                ui,
                if self.playing { Icon::Stop } else { Icon::Play },
                "Play / stop (Space)",
            )
            .clicked()
            {
                self.perform(ShortcutAction::TransportToggle);
            }
            if icon_button(ui, Icon::Play, "Stop and rewind").clicked() {
                self.perform(ShortcutAction::TransportStop);
            }
            // Stateful transport switches show their state through the button
            // fill, so whether the loop, metronome or pattern preview is on is
            // visible without reading the status bar.
            if icon_button_toggle(ui, Icon::Loop, "Loop selection (L)", self.loop_enabled).clicked()
            {
                self.perform(ShortcutAction::ToggleLoop);
            }
            if icon_button_toggle(ui, Icon::Metronome, "Metronome (Ctrl+M)", self.metronome)
                .clicked()
            {
                self.perform(ShortcutAction::ToggleMetronome);
            }
            if icon_button_toggle(
                ui,
                Icon::Solo,
                "Pattern playback (Ctrl+Shift+P)",
                self.selected_clip_playback,
            )
            .clicked()
            {
                self.perform(ShortcutAction::TogglePatternPlayback);
            }
        });
        ui.horizontal(|ui| {
            ui.set_height(ROW_HEIGHT);
            ui.label("BPM");
            let mut bpm = self.project.bpm;
            let bpm_response = ui.add_sized(
                [58.0, ROW_HEIGHT],
                egui::DragValue::new(&mut bpm)
                    .range(20.0..=300.0)
                    .speed(0.5),
            );
            // Tempo is an automation target too, so the numeric box offers a
            // lane the same way a knob does.
            automation_context_menu(
                &bpm_response,
                &format!("Tempo · {bpm:.1} BPM"),
                ParameterTarget::Tempo,
            );
            if bpm_response.changed() {
                self.project.bpm = bpm;
                self.send_audio(AudioCommand::SetTempo(bpm));
            }
            ui.separator();
            let mut numerator = self.project.time_signature.numerator;
            let mut denominator = self.project.time_signature.denominator;
            let n = ui
                .add_sized(
                    [34.0, ROW_HEIGHT],
                    egui::DragValue::new(&mut numerator).range(1..=128),
                )
                .changed();
            ui.label("/");
            let d = ui
                .add_sized(
                    [34.0, ROW_HEIGHT],
                    egui::DragValue::new(&mut denominator).range(1..=128),
                )
                .changed();
            if n || d {
                self.project.time_signature = TimeSignature {
                    numerator,
                    denominator,
                };
                self.sync_project();
            }
            ui.separator();
            ui.label("Snap");
            let mut snap = self.project.snap_ticks;
            ui.scope(|ui| {
                // Keep the combo box inside the 24 px toolbar row.
                ui.spacing_mut().interact_size.y = 20.0;
                ui.spacing_mut().button_padding.y = 0.0;
                egui::ComboBox::from_id_salt("global_snap")
                    .selected_text(snap_label(snap))
                    .width(76.0)
                    .show_ui(ui, |ui| {
                        for ticks in SNAP_STEPS.iter().copied() {
                            ui.selectable_value(&mut snap, ticks, snap_label(ticks));
                        }
                    });
            });
            if snap != self.project.snap_ticks {
                self.project.snap_ticks = snap;
                self.sync_project();
            }
        });
    }

    pub(crate) fn render_clip_picker(&mut self, ui: &mut egui::Ui) {
        ui.heading("Clip picker");
        ui.label("Drag a pattern or sample to the timeline.");
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(280.0)
            .show(ui, |ui| {
                for pattern_id in 0..self.project.tracks.len() {
                    let name = self.project.tracks[pattern_id].name.clone();
                    // The selected pattern is the one the arrangement pen
                    // places, so the picker marks it.
                    let selected = pattern_id == self.selected_clip;
                    let mut button = egui::Button::new(if selected {
                        format!("\u{25B8} [P]  {name}")
                    } else {
                        format!("[P]  {name}")
                    })
                    .sense(egui::Sense::click_and_drag());
                    if selected {
                        button = button.fill(self.ui_accent().gamma_multiply(0.35));
                    }
                    let response = ui.add_sized([ui.available_width(), 32.0], button);
                    if response.clicked() {
                        self.selected_clip = pattern_id;
                        self.arrangement_source = Some(ClipSource::Pattern { pattern_id });
                    }
                    if response.double_clicked() {
                        self.selected_clip = pattern_id;
                        self.windows.piano_roll = true;
                    }
                    if response.drag_started() {
                        self.pattern_drag = Some(ClipSource::Pattern { pattern_id });
                    }
                }
            });
        if ui.button("+ New pattern").clicked() {
            self.selected_clip = self.project.add_pattern();
            self.sync_project();
        }
        ui.separator();
        ui.label("Audio clip");
        ui.add(
            egui::TextEdit::singleline(&mut self.new_audio_path)
                .desired_width(ui.available_width())
                .hint_text("WAV file path"),
        );
        ui.horizontal(|ui| {
            ui.label("Length (steps)");
            ui.add(
                egui::DragValue::new(&mut self.new_audio_length_steps)
                    .range(1..=u64::MAX)
                    .speed(1.0),
            );
        });
        let audio_label = if self.new_audio_path.is_empty() {
            "Set path to drag audio"
        } else {
            "Drag audio clip"
        };
        let audio_response = ui.add_sized(
            [ui.available_width(), 32.0],
            egui::Button::new(audio_label).sense(egui::Sense::click_and_drag()),
        );
        if audio_response.drag_started() && !self.new_audio_path.trim().is_empty() {
            self.pattern_drag = Some(ClipSource::Audio {
                path: self.new_audio_path.trim().to_owned(),
            });
        }
        if audio_response.clicked() && !self.new_audio_path.trim().is_empty() {
            self.arrangement_source = Some(ClipSource::Audio {
                path: self.new_audio_path.trim().to_owned(),
            });
            self.add_audio_clip(self.arrangement_pan_steps, self.selected_lane);
        }
        ui.add_space(12.0);
        if ui.button("+ Arrangement lane").clicked() {
            let old_master_id = self.project.tracks.len();
            self.project.add_lane();
            if self.automation_channel_id == old_master_id {
                self.automation_channel_id = old_master_id + 1;
            }
            self.selected_lane = self.project.tracks.len() - 1;
            self.sync_project();
        }
    }
}
