//! Painting the timeline ruler and the bar/beat grid.

use crate::*;

impl ReaflApp {
    /// Draws the bar ruler above the rows, including the loop marker.
    ///
    /// # Parameters
    ///
    /// - `painter`: the painter of the timeline surface.
    /// - `geo`: the geometry of the current frame.
    pub(crate) fn paint_timeline_ruler(&self, painter: &egui::Painter, geo: TimelineGeometry) {
        painter.text(
            egui::pos2(geo.rect.left() + 16.0, geo.rect.top() + 15.0),
            egui::Align2::LEFT_CENTER,
            "TRACK",
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(145),
        );
        for visible_bar in 0..geo.bars_visible {
            let bar = geo.first_bar.saturating_add(visible_bar);
            let x = geo.left + visible_bar as f32 * geo.bar_width;
            let rect = egui::Rect::from_min_size(
                egui::pos2(x, geo.rect.top()),
                egui::vec2(geo.bar_width, geo.header_height),
            );
            painter.rect_filled(
                rect,
                0.0,
                if bar % 2 == 0 {
                    egui::Color32::from_gray(35)
                } else {
                    egui::Color32::from_gray(29)
                },
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("BAR {}", bar + 1),
                egui::FontId::proportional(11.0),
                egui::Color32::LIGHT_GRAY,
            );
        }
        if let Some((loop_start, loop_end)) = self.arrangement_loop {
            let x_start =
                geo.left + (loop_start as f64 - geo.start_step as f64) as f32 * geo.step_width;
            let x_end =
                geo.left + (loop_end as f64 - geo.start_step as f64) as f32 * geo.step_width;
            let loop_rect = egui::Rect::from_min_max(
                egui::pos2(x_start, geo.rect.top() + 2.0),
                egui::pos2(x_end.max(x_start + 3.0), geo.rect.top() + 7.0),
            );
            painter.rect_filled(loop_rect, 2.0, egui::Color32::from_rgb(112, 208, 190));
        }
    }

    /// Draws the bar and beat lines, the row separators, the playhead and the
    /// empty space below the last row.
    ///
    /// # Parameters
    ///
    /// - `painter`: the painter of the timeline surface.
    /// - `geo`: the geometry of the current frame.
    /// - `rows`: the arrangement layout for the current project.
    pub(crate) fn paint_timeline_grid(
        &self,
        painter: &egui::Painter,
        geo: TimelineGeometry,
        rows: &[RowSpan],
    ) {
        for bar in 0..=geo.bars_visible {
            let x = geo.left + bar as f32 * geo.bar_width;
            painter.line_segment(
                [
                    egui::pos2(x, geo.grid_top),
                    egui::pos2(x, geo.rect.bottom()),
                ],
                egui::Stroke::new(
                    1.0_f32,
                    if bar % 4 == 0 {
                        egui::Color32::from_gray(82)
                    } else {
                        egui::Color32::from_gray(47)
                    },
                ),
            );
        }
        // Beat lines follow the same `geo.step_width` as the clips, so the grid
        // scales with the zoom level instead of drifting away from the content.
        if geo.step_width * geo.steps_per_bar as f32 > 12.0 {
            let beats_visible = geo.bars_visible.saturating_mul(geo.steps_per_bar);
            let beat_width = geo.step_width * 4.0;
            for beat in 0..=beats_visible {
                if beat % 4 == 0 {
                    continue;
                }
                let x = geo.left + beat as f32 * beat_width;
                painter.line_segment(
                    [
                        egui::pos2(x, geo.grid_top),
                        egui::pos2(x, geo.rect.bottom()),
                    ],
                    egui::Stroke::new(1.0_f32, egui::Color32::from_gray(38)),
                );
            }
        }
        let total_height = geo.grid_top + rows.last().map_or(0.0, |span| span.top + span.height);
        for span in rows {
            let y = geo.grid_top + span.top + span.height;
            painter.line_segment(
                [
                    egui::pos2(geo.rect.left(), y),
                    egui::pos2(geo.rect.right(), y),
                ],
                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(48)),
            );
        }
        if self.playing {
            let playhead_step = self.playhead_step();
            let x = geo.left + (playhead_step - geo.start_step as f64) as f32 * geo.step_width;
            if x >= geo.left && x <= geo.rect.right() {
                painter.line_segment(
                    [
                        egui::pos2(x, geo.rect.top()),
                        egui::pos2(x, geo.rect.bottom()),
                    ],
                    egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(255, 104, 79)),
                );
                painter.circle_filled(
                    egui::pos2(x, geo.rect.top() + 4.0),
                    4.0,
                    egui::Color32::from_rgb(255, 104, 79),
                );
            }
        }
        if total_height < geo.rect.bottom() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(geo.rect.left(), total_height),
                    geo.rect.right_bottom(),
                ),
                0.0,
                egui::Color32::from_gray(22),
            );
        }
    }
}
