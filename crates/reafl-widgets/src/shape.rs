//! Drawing helpers for automation points and the curves through them.
//!
//! Lanes, EQ displays and the routing view all draw the same kinds of shape: a
//! series of draggable points and a line through them, or a cable between two
//! channels. Keeping the geometry and the painting here means a lane only has to
//! say *where* its points are, and every view gets identical hit testing and
//! identical markers.

#![warn(missing_docs)]

use eframe::egui;
use reafl_core::project::{AutomationPoint, ThreeBandEq};

/// One point of an editable curve.
///
/// The step is kept as a float so a point can sit between whole steps while it
/// is being dragged; views snap it when they store it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawPoint {
    /// Horizontal position in steps.
    pub step: f64,
    /// Vertical value, in the curve's own units.
    pub value: f32,
}

impl DrawPoint {
    /// Creates a point.
    ///
    /// # Parameters
    ///
    /// - `step`: horizontal position in steps.
    /// - `value`: vertical value.
    ///
    /// # Returns
    ///
    /// The point, ready to be sampled or hit-tested.
    pub fn new(step: f64, value: f32) -> Self {
        Self { step, value }
    }

    /// Converts model points into draw points.
    ///
    /// # Parameters
    ///
    /// - `points`: the stored automation points.
    ///
    /// # Returns
    ///
    /// One [`DrawPoint`] per stored point, in the same order.
    pub fn from_model(points: &[AutomationPoint]) -> Vec<Self> {
        points
            .iter()
            .map(|point| Self::new(point.step as f64, point.value))
            .collect()
    }
}

/// How a point marker is drawn.
#[derive(Clone, Copy, Debug)]
pub struct PointStyle {
    /// Marker radius in points.
    pub radius: f32,
    /// Fill colour.
    pub fill: egui::Color32,
    /// Outline colour.
    pub stroke: egui::Color32,
    /// Outline width; `0.0` skips the outline.
    pub stroke_width: f32,
}

impl Default for PointStyle {
    fn default() -> Self {
        Self {
            radius: 4.5,
            fill: egui::Color32::from_rgb(229, 185, 251),
            stroke: egui::Color32::from_gray(20),
            stroke_width: 1.0,
        }
    }
}

impl PointStyle {
    /// Paints the marker at a screen position.
    ///
    /// # Parameters
    ///
    /// - `painter`: the painter to draw into.
    /// - `position`: screen position of the point.
    pub fn paint(&self, painter: &egui::Painter, position: egui::Pos2) {
        painter.circle_filled(position, self.radius, self.fill);
        if self.stroke_width > 0.0 {
            painter.circle_stroke(
                position,
                self.radius,
                egui::Stroke::new(self.stroke_width, self.stroke),
            );
        }
    }

    /// Tests whether a screen position is close enough to grab the marker.
    ///
    /// # Parameters
    ///
    /// - `pointer`: pointer position.
    /// - `position`: screen position of the point.
    ///
    /// # Returns
    ///
    /// `true` when the pointer is inside the marker's grab radius.
    pub fn hits(&self, pointer: egui::Pos2, position: egui::Pos2) -> bool {
        pointer.distance(position) <= self.radius + 3.0
    }
}

/// A polyline in screen space, used for every curve the editor draws.
#[derive(Clone, Debug, Default)]
pub struct ScreenCurve {
    points: Vec<egui::Pos2>,
}

impl ScreenCurve {
    /// Creates an empty curve.
    ///
    /// # Returns
    ///
    /// A curve with no points yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one sampled point.
    ///
    /// # Parameters
    ///
    /// - `position`: screen position of the sample.
    pub fn push(&mut self, position: egui::Pos2) {
        self.points.push(position);
    }

    /// True when nothing was sampled.
    ///
    /// # Returns
    ///
    /// `true` for an empty curve.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The sampled points.
    ///
    /// # Returns
    ///
    /// The polyline, oldest sample first.
    pub fn points(&self) -> &[egui::Pos2] {
        &self.points
    }

    /// Paints the curve as a stroked line.
    ///
    /// # Parameters
    ///
    /// - `painter`: the painter to draw into.
    /// - `stroke`: colour and width.
    pub fn paint(&self, painter: &egui::Painter, stroke: egui::Stroke) {
        if self.points.len() > 1 {
            painter.add(egui::Shape::line(self.points.clone(), stroke));
        }
    }

    /// Paints the curve filled down to `floor_y`, as a lane's area fill.
    ///
    /// # Parameters
    ///
    /// - `painter`: the painter to draw into.
    /// - `floor_y`: the screen y the fill closes down to.
    /// - `color`: fill colour.
    pub fn paint_filled(&self, painter: &egui::Painter, floor_y: f32, color: egui::Color32) {
        if self.points.len() < 2 {
            return;
        }
        let mut polygon = self.points.clone();
        if let (Some(first), Some(last)) = (self.points.first(), self.points.last()) {
            polygon.push(egui::pos2(last.x, floor_y));
            polygon.push(egui::pos2(first.x, floor_y));
        }
        painter.add(egui::Shape::convex_polygon(
            polygon,
            color,
            egui::Stroke::NONE,
        ));
    }

    /// Paints a cable: a curve that bows away from the straight line.
    ///
    /// # Parameters
    ///
    /// - `painter`: the painter to draw into.
    /// - `stroke`: colour and width of the cable.
    /// - `bow`: how far the middle of the cable is pulled sideways, in points.
    pub fn paint_cable(&self, painter: &egui::Painter, stroke: egui::Stroke, bow: f32) {
        if let (Some(first), Some(last)) = (self.points.first(), self.points.last()) {
            let middle = egui::pos2((first.x + last.x) * 0.5 + bow, (first.y + last.y) * 0.5);
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [*first, middle, middle, *last],
                false,
                egui::Color32::TRANSPARENT,
                stroke,
            ));
        }
    }

    /// Paints the curve segment by segment, blending one colour into another.
    ///
    /// Routing cables use this to fade from the source channel's colour to the
    /// destination's; a single [`paint`](Self::paint) call cannot express that.
    ///
    /// # Parameters
    ///
    /// - `painter`: the painter to draw into.
    /// - `width`: stroke width in points.
    /// - `from`: colour at the start of the curve.
    /// - `to`: colour at the end.
    pub fn paint_gradient(
        &self,
        painter: &egui::Painter,
        width: f32,
        from: egui::Color32,
        to: egui::Color32,
    ) {
        let segments = self.points.len().saturating_sub(1);
        if segments == 0 {
            return;
        }
        for (index, pair) in self.points.windows(2).enumerate() {
            let blend = index as f32 / segments as f32;
            let colour = crate::theme::color_lerp(from, to, blend);
            painter.line_segment([pair[0], pair[1]], egui::Stroke::new(width, colour));
        }
    }
}

/// Samples a value curve into screen space.
///
/// # Parameters
///
/// - `points`: the curve's points, sorted by step.
/// - `steps`: how many samples to take, at least two.
/// - `step_to_x`: maps a step to a screen x.
/// - `value_to_y`: maps a value to a screen y.
///
/// # Returns
///
/// A [`ScreenCurve`] with `steps + 1` samples spanning the curve's own range.
pub fn sample_curve(
    points: &[DrawPoint],
    steps: usize,
    step_to_x: impl Fn(f64) -> f32,
    value_to_y: impl Fn(f32) -> f32,
) -> ScreenCurve {
    let mut curve = ScreenCurve::new();
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return curve;
    };
    let steps = steps.max(2);
    let span = (last.step - first.step).max(f64::EPSILON);
    for index in 0..=steps {
        let step = first.step + span * index as f64 / steps as f64;
        let value = value_at(points, step);
        curve.push(egui::pos2(step_to_x(step), value_to_y(value)));
    }
    curve
}

/// Evaluates a piecewise-linear value through `points` at `step`.
///
/// This is the drawing-side twin of `Project::evaluate_curve`: widgets must not
/// depend on the engine, but a preview has to agree with it closely, so the
/// straight-line case is shared here.
///
/// # Parameters
///
/// - `points`: the curve, sorted by step.
/// - `step`: where to evaluate.
///
/// # Returns
///
/// The interpolated value, clamped to the first and last point outside the
/// curve's range.
pub fn value_at(points: &[DrawPoint], step: f64) -> f32 {
    match points {
        [] => 0.0,
        [only] => only.value,
        _ if step <= points[0].step => points[0].value,
        _ if step >= points[points.len() - 1].step => points[points.len() - 1].value,
        _ => {
            let upper = points.partition_point(|point| point.step <= step);
            let left = points[upper - 1];
            let right = points[upper];
            let distance = (right.step - left.step).max(f64::EPSILON);
            let fraction = ((step - left.step) / distance) as f32;
            left.value + (right.value - left.value) * fraction
        }
    }
}

/// Samples a three-band EQ response for display.
///
/// # Parameters
///
/// - `eq`: the shelf gains in dB.
/// - `width`: width of the display in points.
/// - `height`: height of the display in points.
/// - `samples`: how many points to sample, at least two.
///
/// # Returns
///
/// A [`ScreenCurve`] across the display, with 0 dB on its centre line.
pub fn eq_response_curve(eq: &ThreeBandEq, width: f32, height: f32, samples: usize) -> ScreenCurve {
    let mut curve = ScreenCurve::new();
    let samples = samples.max(2);
    let centre = height * 0.5;
    let scale = centre / 14.0;
    for index in 0..=samples {
        let fraction = index as f32 / samples as f32;
        let low = (-(fraction - 0.12).powi(2) * 40.0).exp();
        let mid = (-(fraction - 0.5).powi(2) * 30.0).exp();
        let high = (-(fraction - 0.88).powi(2) * 40.0).exp();
        let gain = eq.low_db * low + eq.mid_db * mid + eq.high_db * high;
        curve.push(egui::pos2(fraction * width, centre - gain * scale));
    }
    curve
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_at_interpolates_and_clamps() {
        let points = [DrawPoint::new(0.0, 0.0), DrawPoint::new(10.0, 1.0)];
        assert_eq!(value_at(&points, -5.0), 0.0);
        assert!((value_at(&points, 5.0) - 0.5).abs() < 1e-6);
        assert_eq!(value_at(&points, 50.0), 1.0);
        assert_eq!(value_at(&[], 1.0), 0.0);
        assert_eq!(value_at(&[DrawPoint::new(2.0, 0.25)], 9.0), 0.25);
    }

    #[test]
    fn point_style_hit_tests_around_the_marker() {
        let style = PointStyle::default();
        let centre = egui::pos2(10.0, 10.0);
        assert!(style.hits(egui::pos2(11.0, 11.0), centre));
        assert!(!style.hits(egui::pos2(40.0, 40.0), centre));
    }

    #[test]
    fn a_flat_eq_draws_a_straight_centre_line() {
        let curve = eq_response_curve(&ThreeBandEq::default(), 100.0, 40.0, 8);
        assert_eq!(curve.points().len(), 9);
        assert!(curve.points().iter().all(|point| point.y == 20.0));
    }
}
