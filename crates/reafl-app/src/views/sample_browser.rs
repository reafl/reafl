//! The sample browser and the loading helpers it calls.

use crate::*;

impl ReaflApp {
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
        let instrument_id = self.selected_instrument.min(self.project.tracks.len() - 1);
        self.push_history();
        let track = &mut self.project.tracks[instrument_id];
        track.name = default_sample_label(kind).to_owned();
        if let Some(generator) = track.generator_mut() {
            generator.kind = GeneratorKind::Sample(kind);
            generator.sample_path.clear();
        }
        self.sync_project();
        self.load_instrument_samples();
        self.status = format!("{} loaded.", default_sample_label(kind));
    }

    pub(crate) fn load_sample_into_selected(&mut self, path: &str) {
        let instrument_id = self.selected_instrument.min(self.project.tracks.len() - 1);
        self.push_history();
        if let Some(generator) = self.project.tracks[instrument_id].generator_mut() {
            generator.sample_path = path.to_owned();
            generator.kind = GeneratorKind::Sample(DefaultSample::None);
        }
        self.sync_project();
        self.load_instrument_sample(instrument_id);
    }
}
