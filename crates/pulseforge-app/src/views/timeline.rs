//! The arrangement timeline.
//!
//! One frame of the timeline is built in three passes that live in their own
//! modules: [`timeline_paint`](super::timeline_paint) paints the ruler and the
//! grid, [`timeline_rows`](super::timeline_rows) the track headers and clips, and
//! [`timeline_input`](super::timeline_input) turns pointer input into edits.

use crate::*;

/// Geometry of the arrangement timeline for one frame.
///
/// Every pass needs the same origin, zoom and view window, so they are
/// computed once and handed around by value.
#[derive(Clone, Copy)]
pub(crate) struct TimelineGeometry {
    /// The full timeline surface.
    pub(crate) rect: egui::Rect,
    /// Top edge of the row area, below the ruler.
    pub(crate) grid_top: f32,
    /// Left edge of the first bar.
    pub(crate) left: f32,
    /// Height of the bar ruler.
    pub(crate) header_height: f32,
    /// First step that is drawn.
    pub(crate) start_step: u64,
    /// Width of one step in pixels.
    pub(crate) step_width: f32,
    /// Width of one bar in pixels.
    pub(crate) bar_width: f32,
    /// Steps per bar according to the time signature.
    pub(crate) steps_per_bar: u64,
    /// Number of bars that fit on screen.
    pub(crate) bars_visible: u64,
    /// First bar that is drawn.
    pub(crate) first_bar: u64,
    /// One past the last step that is drawn.
    pub(crate) view_end_step: u64,
    /// First arrangement row that is drawn.
    pub(crate) first_lane: usize,
}

impl PulseForgeApp {
    /// Draws the whole arrangement timeline.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI.
    /// - `context`: the frame context.
    pub(crate) fn render_timeline(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8.0, egui::Color32::from_gray(24));
        let geo = self.timeline_geometry(rect);
        let rows = arrangement_layout(&self.project);
        self.paint_timeline_ruler(&painter, geo);
        self.paint_timeline_rows(ui, context, &painter, geo, &rows);
        self.paint_timeline_grid(&painter, geo, &rows);
        self.handle_timeline_input(ui, context, &painter, response, geo, &rows);
    }

    /// Measures the timeline for the given surface.
    ///
    /// # Parameters
    ///
    /// - `rect`: the area the timeline covers.
    ///
    /// # Returns
    ///
    /// The geometry shared by the painting and interaction passes.
    fn timeline_geometry(&self, rect: egui::Rect) -> TimelineGeometry {
        let steps_per_bar = self.project.time_signature.steps_per_bar();
        let step_width = BAR_WIDTH * self.settings.zoom / steps_per_bar as f32;
        let bar_width = BAR_WIDTH * self.settings.zoom;
        let bars_visible = ((rect.width() - LANE_LABEL_WIDTH) / bar_width).ceil() as u64 + 1;
        let first_bar = self.arrangement_pan_steps / steps_per_bar;
        let start_step = first_bar.saturating_mul(steps_per_bar);
        let header_height = 30.0;
        // Rows have independent heights, so the visible count is discovered
        // after the layout is built instead of being divided by a constant.
        let view_end_step =
            start_step.saturating_add(bars_visible.saturating_mul(steps_per_bar));
        TimelineGeometry {
            rect,
            grid_top: rect.top() + header_height,
            left: rect.left() + LANE_LABEL_WIDTH,
            header_height,
            start_step,
            step_width,
            bar_width,
            steps_per_bar,
            bars_visible,
            first_bar,
            view_end_step,
            first_lane: self.arrangement_pan_lanes,
        }
    }
}

