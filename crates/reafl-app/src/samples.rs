//! Loading samples, waveform peaks and audio clips.

use crate::*;

impl ReaflApp {
    /// Peak envelope for `path`, computed once and cached per path.
    pub(crate) fn peaks_for(&mut self, path: &str, buckets: usize) -> Option<Vec<f32>> {
        if let Some((cached_path, peaks)) = &self.sample_peaks
            && cached_path == path
            && peaks.len() == buckets
        {
            return Some(peaks.clone());
        }
        let sample = audio::read_wav(path).ok()?;
        let frames = sample.frames.len();
        if frames == 0 {
            return None;
        }
        let width = buckets.max(1);
        let mut peaks = vec![0.0_f32; width];
        for (index, peak) in peaks.iter_mut().enumerate() {
            let start = index * frames / width;
            let end = ((index + 1) * frames / width).max(start + 1).min(frames);
            *peak = sample.frames[start..end]
                .iter()
                .map(|frame| frame[0].abs().max(frame[1].abs()))
                .fold(0.0_f32, f32::max);
        }
        self.sample_peaks = Some((path.to_owned(), peaks.clone()));
        Some(peaks)
    }

    /// Draws a cached waveform for an instrument's sample.
    pub(crate) fn draw_sample_waveform(
        &mut self,
        painter: egui::Painter,
        rect: egui::Rect,
        instrument_id: usize,
    ) {
        let path = self
            .project
            .tracks
            .get(instrument_id)
            .and_then(|track| track.generator())
            .map(|generator| generator.sample_path.clone())
            .unwrap_or_default();
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(20));
        if path.trim().is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No sample loaded",
                egui::FontId::proportional(11.0),
                egui::Color32::from_gray(120),
            );
            return;
        }
        let buckets = (rect.width() / 2.0).max(16.0) as usize;
        let Some(peaks) = self.peaks_for(&path, buckets) else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Sample could not be read",
                egui::FontId::proportional(11.0),
                egui::Color32::LIGHT_RED,
            );
            return;
        };
        let color = self.instrument_color(instrument_id);
        let center = rect.center().y;
        for (index, peak) in peaks.iter().enumerate() {
            let x = rect.left() + index as f32 * 2.0;
            let half = (peak * rect.height() * 0.45).max(0.5);
            painter.line_segment(
                [egui::pos2(x, center - half), egui::pos2(x, center + half)],
                egui::Stroke::new(1.0_f32, color),
            );
        }
    }
}
