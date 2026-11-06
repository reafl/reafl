//! The per-frame update loop: input, audio commands and the window wiring.

use crate::*;

impl eframe::App for PulseForgeApp {
    fn update(&mut self, context: &egui::Context, frame: &mut eframe::Frame) {
        // Tag this thread and wrap the frame so the flame graph shows the UI
        // channel; nesting rank below this span is the render cost per widget
        // group that is instrumented explicitly.
        set_thread_channel(Channel::Ui);
        profile_scope!("ui frame");
        // Background results arrive here, before anything reads the state.
        self.poll_tasks();
        // Plug-in editors are native child windows, so the DAW window's handle
        // and scale factor are needed to place them under their egui wrapper.
        self.last_pixels_per_point = context.pixels_per_point();
        if self.main_hwnd == 0
            && let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(frame)
            && let raw_window_handle::RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            self.main_hwnd = win32.hwnd.get();
        }
        if let Some(audio) = &self.audio {
            audio.collect_retired_projects();
        }
        // Sampled once per frame; every enable/selection gesture reads these.
        let (shift, ctrl) = context.input(|input| {
            (
                input.modifiers.shift,
                input.modifiers.command || input.modifiers.ctrl,
            )
        });
        self.mod_shift = shift;
        self.mod_ctrl = ctrl;
        // Re-applying visuals every frame would be wasteful, so the theme is
        // pushed only when the settings differ from what is already active.
        let theme_key = egui::Id::new("applied_theme_stamp");
        let stamp = format!("{:?}", self.settings.theme);
        let applied: Option<String> = context.data(|data| data.get_temp(theme_key));
        if applied.as_deref() != Some(stamp.as_str()) {
            apply_theme(context, &self.settings.theme);
            context.data_mut(|data| data.insert_temp(theme_key, stamp));
        }
        // DPI-like interface scale, pushed only when it changes so the layout
        // is not invalidated every frame.
        let scale_key = egui::Id::new("applied_ui_scale");
        let applied_scale: Option<f32> = context.data(|data| data.get_temp(scale_key));
        if applied_scale != Some(self.settings.ui_scale) {
            context.set_zoom_factor(self.settings.ui_scale);
            context.data_mut(|data| data.insert_temp(scale_key, self.settings.ui_scale));
        }
        self.advance_playhead();
        self.poll_last_tweaked();
        self.flush_project();
        let dt = context
            .input(|input| input.stable_dt)
            .clamp(1.0 / 240.0, 0.1);
        self.last_frame_seconds = dt;
        let animations = self.settings.animations;
        let speed = self.settings.animation_speed;
        let lift_target = if matches!(self.selection, Some(Selection::Clip { .. })) {
            1.0
        } else {
            0.0
        };
        self.clip_lift.set(lift_target);
        self.drag_ghost.set(if self.pattern_drag.is_some() {
            1.0
        } else {
            0.0
        });
        self.inspector_fade
            .set(if self.windows.mixer { 1.0 } else { 0.0 });
        self.clip_lift.step(dt, animations, speed);
        self.drag_ghost.step(dt, animations, speed);
        self.inspector_fade.step(dt, animations, speed);
        if self.clip_lift.is_animating()
            || self.drag_ghost.is_animating()
            || self.inspector_fade.is_animating()
        {
            context.request_repaint();
        }
        let automated_targets: Vec<ParameterTarget> = self
            .project
            .automation_tracks
            .iter()
            .filter(|track| track.enabled)
            .map(|track| track.target)
            .collect();
        let automated_values = self.live_automated_values();
        context.data_mut(|data| {
            data.insert_temp(egui::Id::new(AUTOMATED_TARGETS_KEY), automated_targets);
            data.insert_temp(egui::Id::new(AUTOMATED_VALUES_KEY), automated_values);
        });
        let preview_pattern = self
            .selected_pattern_playback
            .then_some(self.selected_pattern);
        if preview_pattern != self.audio_preview_pattern {
            self.audio_preview_pattern = preview_pattern;
            self.send_audio(AudioCommand::SetSelectedPattern(preview_pattern));
        }
        let loop_range = self.loop_enabled.then_some(self.arrangement_loop).flatten();
        if loop_range != self.audio_loop_range {
            self.audio_loop_range = loop_range;
            self.send_audio(AudioCommand::SetLoop(loop_range));
        }
        let automation_request = context.data_mut(|data| {
            data.remove_temp::<ParameterTarget>(egui::Id::new("knob_add_automation"))
        });
        if let Some(target) = automation_request {
            // Focus the picker on the parameter the control asked for, then let
            // the shared helper create the lane. Instrument and insert targets
            // used to be pushed here as well, which made the global ones create
            // two lanes.
            self.focus_automation_target(target);
            self.create_automation_for_target(target);
            self.windows.automation = true;
        }
        if !context.wants_keyboard_input() {
            let actions = self.pressed_actions(context);
            for action in actions {
                self.perform(action);
            }
        }
        egui::TopBottomPanel::top("toolbar")
            .exact_height(60.0)
            .show(context, |ui| self.render_toolbar(ui));
        egui::TopBottomPanel::bottom("footer").show(context, |ui| self.render_footer(ui));

        egui::SidePanel::left("clip_picker")
            .default_width(245.0)
            .min_width(190.0)
            .max_width(360.0)
            .resizable(true)
            .show(context, |ui| self.render_clip_picker(ui));

        egui::CentralPanel::default().show(context, |ui| {
            self.render_arrangement(ui, context);
        });

        let mut rack_open = self.windows.channel_rack;
        egui::Window::new("Channel Rack · Step Sequencer")
            .open(&mut rack_open)
            .default_size([1050.0, 290.0])
            .min_size([520.0, 220.0])
            .resizable(true)
            .show(context, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.render_channel_rack(ui, context));
            });
        self.windows.channel_rack = rack_open;

        let mut piano_open = self.windows.piano_roll;
        egui::Window::new("Piano Roll")
            .open(&mut piano_open)
            .default_size([1150.0, 690.0])
            .min_size([520.0, 320.0])
            .resizable(true)
            .show(context, |ui| {
                self.render_piano_roll(ui, context);
            });
        self.windows.piano_roll = piano_open;

        let mut mixer_open = self.windows.mixer;
        egui::Window::new("Mixer")
            .open(&mut mixer_open)
            .default_size([1120.0, 640.0])
            .min_size([520.0, 320.0])
            .resizable(true)
            .show(context, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.render_mixer(ui));
            });
        self.windows.mixer = mixer_open;

        let mut routing_open = self.windows.routing;
        egui::Window::new("Mixer Routing")
            .open(&mut routing_open)
            .default_size([900.0, 620.0])
            .min_size([520.0, 340.0])
            .resizable(true)
            .show(context, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.render_routing_cables(ui, context));
            });
        self.windows.routing = routing_open;

        let mut midi_open = self.windows.midi_routing;
        egui::Window::new("MIDI Routing")
            .open(&mut midi_open)
            .default_size([470.0, 600.0])
            .min_size([360.0, 300.0])
            .resizable(true)
            .show(context, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.render_midi(ui));
            });
        self.windows.midi_routing = midi_open;

        let mut automation_open = self.windows.automation;
        egui::Window::new("Plugin Automation")
            .open(&mut automation_open)
            .default_size([740.0, 560.0])
            .min_size([480.0, 320.0])
            .resizable(true)
            .show(context, |ui| self.render_automation_window(ui));
        self.windows.automation = automation_open;

        let mut samples_open = self.windows.samples;
        egui::Window::new("Sample Browser")
            .open(&mut samples_open)
            .default_size([420.0, 420.0])
            .min_size([300.0, 220.0])
            .resizable(true)
            .show(context, |ui| self.render_sample_browser(ui));
        self.windows.samples = samples_open;

        if let Some(lane) = self.renaming_track {
            let mut commit = false;
            let mut cancel = false;
            egui::Window::new("Rename track")
                .id(egui::Id::new("rename_track_window"))
                .collapsible(false)
                .resizable(false)
                .show(context, |ui| {
                    ui.label(format!(
                        "Track {} and its mixer channel share one name.",
                        lane + 1
                    ));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.rename_buffer)
                            .desired_width(260.0)
                            .hint_text("Track name"),
                    );
                    response.request_focus();
                    if response.lost_focus()
                        && context.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        commit = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Rename").clicked() {
                            commit = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if commit {
                self.commit_rename_track();
            } else if cancel {
                self.renaming_track = None;
            }
        }

        let mut settings_open = self.windows.settings;
        egui::Window::new("Settings")
            .open(&mut settings_open)
            .default_size([720.0, 560.0])
            .min_size([520.0, 340.0])
            .resizable(true)
            .show(context, |ui| self.render_settings_window(ui));
        self.windows.settings = settings_open;

        let mut render_open = self.windows.render;
        egui::Window::new("WAV Render")
            .open(&mut render_open)
            .default_size([480.0, 160.0])
            .min_size([360.0, 120.0])
            .resizable(true)
            .show(context, |ui| self.render_export_window(ui));
        self.windows.render = render_open;

        self.render_plugin_windows(context);
        self.render_debug_panel(context);

        let callback_error = match self.midi_callback_error.lock() {
            Ok(mut error) => error.take(),
            Err(_) => Some("MIDI callback error status unavailable.".to_owned()),
        };
        if let Some(error) = callback_error {
            self.status = error;
        }
        context.request_repaint_after(if self.playing {
            std::time::Duration::from_millis(8)
        } else {
            std::time::Duration::from_millis(50)
        });
    }
}
