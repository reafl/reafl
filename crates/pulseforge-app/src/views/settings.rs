//! The settings window and its tabs.

use crate::*;

impl PulseForgeApp {
    /// Settings window. Every page edits [`Settings`] directly, and changes that
    /// affect the audio engine are pushed as soon as they are made.
    pub(crate) fn render_settings_window(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (SettingsTab::Shortcuts, "Shortcuts"),
                (SettingsTab::Plugins, "Plug-ins"),
                (SettingsTab::Samples, "Samples"),
                (SettingsTab::Interface, "Interface"),
                (SettingsTab::Graphics, "Graphics"),
                (SettingsTab::Theming, "Theming"),
                (SettingsTab::General, "General"),
            ] {
                if ui
                    .selectable_label(self.settings_tab == tab, label)
                    .clicked()
                {
                    self.settings_tab = tab;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Search settings");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings_search)
                    .desired_width(220.0)
                    .hint_text("Option name"),
            );
            if ui.small_button("Clear").clicked() {
                self.settings_search.clear();
            }
        });
        ui.separator();
        match self.settings_tab {
            SettingsTab::Shortcuts => self.render_shortcut_settings(ui),
            SettingsTab::Plugins => self.render_plugin_settings(ui),
            SettingsTab::Samples => self.render_sample_settings(ui),
            SettingsTab::Interface => self.render_interface_settings(ui),
            SettingsTab::Graphics => self.render_graphics_settings(ui),
            SettingsTab::Theming => self.render_theming_settings(ui),
            SettingsTab::General => self.render_general_settings(ui),
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Save settings").clicked() {
                self.save_settings();
            }
            if ui.button("Reload").clicked() {
                self.settings = Settings::load(&self.settings_path);
                self.apply_audio_settings();
                self.status = "Settings reloaded.".to_owned();
            }
            ui.label(format!("File: {}", self.settings_path));
        });
    }

    /// Editable key map. Clicking a chord arms capture; the next key press is
    /// assigned through `Settings::bind`, which is the same API code uses.
    pub(crate) fn render_shortcut_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(
            "Click a chord and press the keys you want. Bindings are stored in settings.json \
             and can also be created programmatically with Settings::bind.",
        );
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(
                egui::TextEdit::singleline(&mut self.shortcut_search)
                    .desired_width(220.0)
                    .hint_text("Command or chord"),
            );
            if ui.button("Restore defaults").clicked() {
                self.settings.reset_shortcuts();
                self.status = "Shortcut map restored.".to_owned();
            }
        });
        ui.separator();
        let capturing = self.capturing_shortcut;
        // Groups are collapsible: each header is a collapsing header holding its
        // own grid, so a long key map stays navigable.
        let filter = self.shortcut_search.to_ascii_lowercase();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut current_group = "";
                let mut group_actions: Vec<ShortcutAction> = Vec::new();
                let mut groups: Vec<(&'static str, Vec<ShortcutAction>)> = Vec::new();
                for action in ShortcutAction::all() {
                    let group = action.group();
                    if group != current_group {
                        if !current_group.is_empty() {
                            groups.push((current_group, std::mem::take(&mut group_actions)));
                        }
                        current_group = group;
                    }
                    group_actions.push(*action);
                }
                if !current_group.is_empty() {
                    groups.push((current_group, group_actions));
                }
                for (group, actions) in groups {
                    let visible: Vec<ShortcutAction> = actions
                        .into_iter()
                        .filter(|action| {
                            if filter.is_empty() {
                                return true;
                            }
                            let keys = self.settings.keys_for(*action);
                            action.label().to_ascii_lowercase().contains(&filter)
                                || keys.to_ascii_lowercase().contains(&filter)
                                || group.to_ascii_lowercase().contains(&filter)
                        })
                        .collect();
                    if visible.is_empty() {
                        continue;
                    }
                    let header = egui::RichText::new(format!("{group} ({})", visible.len()))
                        .strong()
                        .color(self.ui_accent());
                    egui::CollapsingHeader::new(header)
                        .id_salt(("shortcut_group", group))
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::Grid::new(("shortcut_grid", group))
                                .num_columns(3)
                                .striped(true)
                                .show(ui, |ui| {
                                    for action in visible {
                                        let keys = self.settings.keys_for(action);
                                        let label = if capturing == Some(action) {
                                            "Press keys?".to_owned()
                                        } else if keys.is_empty() {
                                            "Unbound".to_owned()
                                        } else {
                                            keys.to_owned()
                                        };
                                        ui.label(action.label());
                                        if ui
                                            .add_sized(
                                                [150.0, 20.0],
                                                egui::Button::new(label)
                                                    .selected(capturing == Some(action)),
                                            )
                                            .clicked()
                                        {
                                            self.capturing_shortcut = Some(action);
                                            self.shortcut_draft.clear();
                                        }
                                        if ui.small_button("Clear").clicked() {
                                            self.settings.unbind(action);
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                }
            });
        if capturing.is_some()
            && let Some(chord) = Self::capture_chord(ui.ctx())
        {
            if let Some(action) = self.capturing_shortcut.take() {
                let conflicts: Vec<ShortcutAction> = self
                    .settings
                    .actions_for_chord(&chord.display())
                    .into_iter()
                    .filter(|other| *other != action)
                    .collect();
                self.settings.bind(action, &chord.display());
                self.status = if conflicts.is_empty() {
                    format!("Bound {} to {}.", action.label(), chord.display())
                } else {
                    format!(
                        "Bound {} to {}; this also matches {}.",
                        action.label(),
                        chord.display(),
                        conflicts
                            .iter()
                            .map(|conflict| conflict.label())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
            }
        }
    }

    /// Reads the first key chord in the current frame, used by the capture UI.
    pub(crate) fn capture_chord(context: &egui::Context) -> Option<Chord> {
        context.input(|input| {
            let mut chord = Chord {
                ctrl: input.modifiers.ctrl,
                shift: input.modifiers.shift,
                alt: input.modifiers.alt,
                key: None,
            };
            for event in &input.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } = event
                {
                    let name = key.name();
                    if let Some(canonical) = settings::KEY_NAMES
                        .iter()
                        .find(|candidate| candidate.eq_ignore_ascii_case(name))
                    {
                        chord.key = Some(*canonical);
                        return Some(chord);
                    }
                }
            }
            None
        })
    }

    pub(crate) fn render_plugin_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("Folders searched for installed plug-ins. They appear in the plug-in picker.");
        Self::path_list_editor(ui, "VST 2", &mut self.settings.vst_paths);
        Self::path_list_editor(ui, "VST3", &mut self.settings.vst3_paths);
        Self::path_list_editor(ui, "CLAP", &mut self.settings.clap_paths);
        ui.separator();
        if ui.button("Rescan plug-ins").clicked() {
            self.rescan_plugins();
        }
        if let Some(error) = &self.plugin_scan_error {
            ui.colored_label(egui::Color32::from_rgb(228, 178, 92), error);
        }
        ui.label(format!(
            "{} plug-in(s) discovered",
            self.external_plugins.len()
        ));
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .show(ui, |ui| {
                for plugin in &self.external_plugins {
                    ui.label(format!("{}  ?  {}", plugin.label(), plugin.path));
                }
            });
        ui.separator();
        ui.label(
            "VST3 and CLAP bundles are listed here and can be inserted on any mixer slot or \
             channel. Inserts process audio in 64-frame blocks (about 1.3 ms of latency at \
             48 kHz) and generators accept notes and pitch bend from the sequencer. Opening a \
             hosted plug-in attaches its own editor window.",
        );
    }

    pub(crate) fn render_sample_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("Folders listed in the sample browser.");
        Self::path_list_editor(ui, "Samples", &mut self.settings.sample_paths);
        if ui.button("Rescan samples").clicked() {
            self.refresh_sample_files();
        }
        ui.label(format!("{} sample(s) found", self.sample_files.len()));
    }

    pub(crate) fn render_interface_settings(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.settings.animations,
            "Animate interface transitions",
        );
        ui.add(
            egui::Slider::new(&mut self.settings.animation_speed, 0.1..=4.0)
                .text("Animation speed"),
        );
        ui.separator();
        let mut smoothing = self.settings.parameter_smoothing;
        ui.checkbox(&mut smoothing, "Smooth parameter changes (linear ramp)");
        let ramp_changed = ui
            .add(
                egui::Slider::new(&mut self.settings.smoothing_ms, 0.0..=200.0)
                    .text("Ramp length (ms)"),
            )
            .changed();
        let toggled = smoothing != self.settings.parameter_smoothing;
        if toggled {
            self.settings.parameter_smoothing = smoothing;
        }
        if ramp_changed || toggled {
            self.apply_audio_settings();
        }
        ui.separator();
        ui.add(
            egui::Slider::new(&mut self.settings.default_track_height, 34.0..=200.0)
                .text("New track height"),
        );
        if ui.button("Apply height to all tracks").clicked() {
            let height = self.settings.default_track_height;
            self.push_history();
            for lane in &mut self.project.lanes {
                lane.height = height;
            }
            self.sync_project();
        }
        ui.add(
            egui::Slider::new(&mut self.settings.default_velocity, 1..=127)
                .text("New note velocity"),
        );
        ui.add(
            egui::Slider::new(&mut self.settings.metronome_volume, 0.0..=1.0)
                .text("Metronome level"),
        );
    }

    pub(crate) fn render_graphics_settings(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.settings.animations,
            "Animate interface transitions",
        );
        ui.checkbox(
            &mut self.settings.fancy_animations,
            "Fancy animations (bounce and overshoot)",
        );
        ui.add(
            egui::Slider::new(&mut self.settings.animation_speed, 0.1..=4.0)
                .text("Animation speed"),
        );
        ui.separator();
        ui.add(egui::Slider::new(&mut self.settings.zoom, MIN_ZOOM..=MAX_ZOOM).text("Arrangement zoom"));
        ui.add(
            egui::Slider::new(&mut self.settings.wheel_step_ticks, 1..=32)
                .text("Alt+wheel step (ticks)"),
        );
        ui.add(
            egui::Slider::new(&mut self.settings.ui_scale, 0.6..=2.5)
                .text("Interface scale")
                .suffix(" x"),
        );
        ui.label("Ctrl+wheel zooms the arrangement and editors; Alt+wheel changes the step size.");
        ui.label("Ctrl+click the ruler to reset the zoom; the interface scale works like a DPI setting.");
        ui.separator();
        ui.label("Tooltips");
        for mode in settings::TooltipMode::ALL {
            ui.radio_value(&mut self.settings.tooltips, mode, mode.label());
        }
        if self.settings.tooltips == settings::TooltipMode::Hover {
            ui.add(
                egui::Slider::new(&mut self.settings.tooltip_delay, 0.0..=3.0)
                    .text("Hover delay (s)"),
            );
        }
    }

    pub(crate) fn render_theming_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(
            "A theme only changes appearance: colours used by meters, clips, MIDI notes and \
             panels, plus the font scale and an optional background image.",
        );
        ui.horizontal(|ui| {
            ui.label("Preset");
            for preset in settings::Theme::presets() {
                if ui.button(&preset.name).clicked() {
                    self.settings.theme = preset;
                }
            }
        });
        ui.separator();
        egui::Grid::new("theme_colors")
            .num_columns(2)
            .show(ui, |ui| {
                let mut swatch = |label: &str, channels: &mut [u8; 3]| {
                    ui.label(label);
                    let mut color = egui::Color32::from_rgb(channels[0], channels[1], channels[2]);
                    if ui.color_edit_button_srgba(&mut color).changed() {
                        *channels = [color.r(), color.g(), color.b()];
                    }
                    ui.end_row();
                };
                let theme = &mut self.settings.theme;
                swatch("Accent", &mut theme.accent);
                swatch("Background", &mut theme.background);
                swatch("Panel", &mut theme.panel);
                swatch("MIDI note / clip", &mut theme.note);
                swatch("Meter low", &mut theme.meter_low);
                swatch("Meter mid", &mut theme.meter_mid);
                swatch("Meter high", &mut theme.meter_high);
                swatch("Text", &mut theme.text);
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Font scale");
            ui.add(
                egui::Slider::new(&mut self.settings.theme.font_scale, 0.75..=1.6)
                    .fixed_decimals(2),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Background image");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.theme.background_image)
                    .desired_width(280.0)
                    .hint_text("Path to a PNG/JPG used behind the arrangement"),
            );
            if ui.button("Clear").clicked() {
                self.settings.theme.background_image.clear();
            }
        });
        if ui.button("Reset theme").clicked() {
            self.settings.theme = settings::Theme::default();
        }
    }

    pub(crate) fn render_general_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("Placeholder page for general options.");
        ui.checkbox(
            &mut self.settings.restore_last_project,
            "Reopen the last project on startup",
        );
        ui.add(
            egui::Slider::new(&mut self.settings.autosave_minutes, 0..=120)
                .text("Autosave every (minutes, 0 = off)"),
        );
        ui.add(
            egui::Slider::new(&mut self.settings.metronome_volume, 0.0..=1.0)
                .text("Metronome level"),
        );
    }

    /// Shared editor for a list of directories.
    pub(crate) fn path_list_editor(ui: &mut egui::Ui, title: &str, paths: &mut Vec<String>) {
        ui.label(egui::RichText::new(title).strong());
        let mut remove = None;
        for (index, path) in paths.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(path)
                        .desired_width(320.0)
                        .hint_text("Folder path"),
                );
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            paths.remove(index);
        }
        if ui.button("Add folder").clicked() {
            paths.push(String::new());
        }
        ui.add_space(6.0);
    }

    /// Sample browser: a channel-rack-shaped list without the step sequencer.
    pub(crate) fn render_sample_browser(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.sample_tab == SampleTab::Builtin, "Built-in")
                .clicked()
            {
                self.sample_tab = SampleTab::Builtin;
            }
            if ui
                .selectable_label(self.sample_tab == SampleTab::Folders, "Folders")
                .clicked()
            {
                self.sample_tab = SampleTab::Folders;
                if self.sample_files.is_empty() {
                    self.refresh_sample_files();
                }
            }
            if ui.button("Rescan").clicked() {
                self.refresh_sample_files();
            }
            if ui.button("Settings").clicked() {
                self.windows.settings = true;
                self.settings_tab = SettingsTab::Samples;
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.sample_tab {
                SampleTab::Builtin => {
                    for kind in [
                        DefaultSample::Kick,
                        DefaultSample::Snare,
                        DefaultSample::HiHat,
                    ] {
                        ui.horizontal(|ui| {
                            if ui
                                .add_sized(
                                    [180.0, 22.0],
                                    egui::Button::new(default_sample_label(kind)),
                                )
                                .clicked()
                            {
                                self.load_default_sample_into_selected(kind);
                            }
                            ui.label("adds to the selected instrument");
                        });
                    }
                }
                SampleTab::Folders => {
                    if self.sample_files.is_empty() {
                        ui.label("No folders configured in Settings ? Samples.");
                    }
                    for path in self.sample_files.clone() {
                        ui.horizontal(|ui| {
                            let name = path
                                .rsplit(['/', '\\'])
                                .next()
                                .unwrap_or(path.as_str())
                                .to_owned();
                            if ui
                                .add_sized([220.0, 20.0], egui::Button::new(name))
                                .clicked()
                            {
                                self.preview_sample(&path);
                            }
                            if ui
                                .small_button("Load")
                                .on_hover_text("Load into the selected instrument")
                                .clicked()
                            {
                                self.load_sample_into_selected(&path);
                            }
                        });
                    }
                }
            });
    }

    /// Assigns a built-in drum sample to the selected instrument.
    pub(crate) fn load_default_sample_into_selected(&mut self, kind: DefaultSample) {
        let instrument_id = self
            .selected_instrument
            .min(self.project.instruments.len() - 1);
        self.push_history();
        let instrument = &mut self.project.instruments[instrument_id];
        instrument.default_sample = kind;
        instrument.sample_path.clear();
        instrument.name = default_sample_label(kind).to_owned();
        self.sync_project();
        self.load_instrument_samples();
        self.status = format!("{} loaded.", default_sample_label(kind));
    }

    pub(crate) fn load_sample_into_selected(&mut self, path: &str) {
        let instrument_id = self
            .selected_instrument
            .min(self.project.instruments.len() - 1);
        self.push_history();
        self.project.instruments[instrument_id].sample_path = path.to_owned();
        self.project.instruments[instrument_id].default_sample = DefaultSample::None;
        self.sync_project();
        self.load_instrument_sample(instrument_id);
    }
}

