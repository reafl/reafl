//! The offline export window and the audio sample-clip window.

use crate::*;

impl ReaflApp {
    pub(crate) fn render_export_window(&mut self, ui: &mut egui::Ui) {
        ui.label("Render the arrangement offline to stereo 16-bit PCM WAV.");
        ui.horizontal(|ui| {
            ui.label("Output path");
            ui.text_edit_singleline(&mut self.render_path);
        });
        if ui.button("Render").clicked() {
            let path = self.render_path.trim().to_owned();
            if path.is_empty() {
                self.status = "Enter an output path before rendering.".to_owned();
            } else {
                match audio::render_project_to_wav(&self.project, &path) {
                    Ok(frames) => {
                        self.status = format!("Rendered {frames} frames to {path}.");
                    }
                    Err(error) => {
                        self.status = format!("WAV render failed: {error}");
                    }
                }
            }
        }
    }

    /// Sample clip window: file identity, waveform and both the project-wide
    /// and per-clip playback controls, shown side by side.
    pub(crate) fn render_sample_clip_window(
        &mut self,
        ui: &mut egui::Ui,
        lane: usize,
        clip: usize,
    ) {
        let Some(source_path) = self
            .project
            .tracks
            .get(lane)
            .and_then(|lane| lane.clips.get(clip))
            .and_then(|clip| clip.audio_path.clone())
        else {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "That clip is no longer an audio clip.",
            );
            return;
        };
        let file_name = source_path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&source_path)
            .to_owned();
        let stem = file_name
            .rsplit_once('.')
            .map_or(file_name.clone(), |(stem, _)| stem.to_owned());
        let (frames, sample_rate) = audio::read_wav(&source_path)
            .map(|sample| (sample.frames.len(), sample.sample_rate))
            .unwrap_or((0, 48_000));
        let seconds = if sample_rate > 0 {
            frames as f64 / f64::from(sample_rate)
        } else {
            0.0
        };
        ui.horizontal(|ui| {
            ui.heading(stem.clone());
            ui.label(
                egui::RichText::new(&file_name)
                    .small()
                    .color(egui::Color32::from_gray(150)),
            );
        });
        ui.label(format!(
            "{frames} frames \u{00B7} {seconds:.2} s \u{00B7} {sample_rate} Hz"
        ));
        let peaks = self.peaks_for(&source_path, 320);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(320.0), 110.0),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(18));
        if let Some(peaks) = peaks {
            let mid = rect.center().y;
            let width = rect.width() / peaks.len().max(1) as f32;
            for (index, peak) in peaks.iter().enumerate() {
                let x = rect.left() + index as f32 * width;
                let height = (peak.clamp(0.0, 1.0) * rect.height() * 0.45).max(1.0);
                painter.line_segment(
                    [egui::pos2(x, mid - height), egui::pos2(x, mid + height)],
                    egui::Stroke::new(width.max(1.0), egui::Color32::from_rgb(122, 102, 193)),
                );
            }
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waveform unavailable",
                egui::FontId::proportional(12.0),
                egui::Color32::from_gray(140),
            );
        }
        ui.separator();
        let mut changed = false;
        // Both the defaults and the per-clip controls are automatable; the
        // defaults address `SampleDefaults`, the clip controls their clip.
        let default_target =
            |parameter: project::ClipParameter| ParameterTarget::SampleDefaults { parameter };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("All samples").strong());
                changed |= Knob::new("Volume", 0.0, 1.0, "")
                    .with_default(0.8)
                    .automates(default_target(project::ClipParameter::Gain))
                    .show(ui, &mut self.project.sample_gain);
                changed |= Knob::new("Pan", -1.0, 1.0, "")
                    .with_default(0.0)
                    .automates(default_target(project::ClipParameter::Pan))
                    .show(ui, &mut self.project.sample_pan);
                changed |= Knob::new("Pitch", -24.0, 24.0, " st")
                    .with_default(0.0)
                    .automates(default_target(project::ClipParameter::Pitch))
                    .show(ui, &mut self.project.sample_pitch);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("This clip").strong());
                let clip_target = |parameter: project::ClipParameter| ParameterTarget::Clip {
                    track_id: lane,
                    clip_id: clip,
                    parameter,
                };
                if let Some(clip) = self
                    .project
                    .tracks
                    .get_mut(lane)
                    .and_then(|lane| lane.clips.get_mut(clip))
                {
                    changed |= Knob::new("Volume", 0.0, 1.0, "")
                        .with_default(0.8)
                        .automates(clip_target(project::ClipParameter::Gain))
                        .show(ui, &mut clip.sample_gain);
                    changed |= Knob::new("Pan", -1.0, 1.0, "")
                        .with_default(0.0)
                        .automates(clip_target(project::ClipParameter::Pan))
                        .show(ui, &mut clip.sample_pan);
                    changed |= Knob::new("Pitch", -24.0, 24.0, " st")
                        .with_default(0.0)
                        .automates(clip_target(project::ClipParameter::Pitch))
                        .show(ui, &mut clip.sample_pitch);
                }
            });
        });
        if changed {
            self.sync_project();
        }
    }
}
