//! One control type for every editable value in the editor.
//!
//! The editor used to grow a bespoke widget per parameter: a knob here, a raw
//! `egui::Slider` there, a combo box with hand-written automation buttons next
//! to it. [`ValueControl`] folds those into a single description of *what* is
//! being edited - the value behind it, the range, whether it is a knob, a
//! slider, a number box, a toggle or a choice - and takes care of the parts
//! every control shares:
//!
//! - the value lives behind a `&mut` binding, so drawing and editing cannot
//!   disagree about where the truth is;
//! - an optional [`ParameterTarget`] turns the control into an automation
//!   target, including the automated ring, the live engine value and the
//!   "Add automation lane..." entry;
//! - an optional callback sees every accepted change, for the cases where the
//!   value has to be pushed somewhere as soon as it moves.
//!
//! ```no_run
//! # use pulseforge_widgets::control::ValueControl;
//! # fn demo(ui: &mut eframe::egui::Ui, gain: &mut f32) {
//! let edit = ValueControl::knob("Gain", gain, 0.0..=1.0).show(ui);
//! if edit.changed {
//!     // The new value is already in `gain`; push it where it belongs.
//! }
//! # }
//! ```

use eframe::egui;
use pulseforge_core::project::ParameterTarget;

use crate::automation_bridge::{
    automation_context_menu, automation_lane_button, request_automation_lane, target_live_value,
};
use crate::knob::Knob;

/// How a [`ValueControl`] draws itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    /// Rotary knob; the densest control, used in racks and strips.
    Knob,
    /// Horizontal slider with its label to the right.
    Slider,
    /// Drag value for numbers that are typed more often than dragged.
    Number,
    /// Checkbox for a flag.
    Toggle,
    /// Drop-down for an enum, addressed as an index.
    Choice,
}

/// The value a control edits.
///
/// Choices are stored as an index so an enum can be automated as an integer.
pub enum Bound<'a> {
    /// A continuous value.
    Number(&'a mut f32),
    /// A flag, edited as on or off.
    Flag(&'a mut bool),
    /// A choice between `labels`, addressed by index.
    Choice {
        /// Selected entry.
        index: &'a mut usize,
        /// Every entry, in display order.
        labels: &'a [&'a str],
    },
}

/// What a control was given to draw.
#[derive(Debug)]
pub struct ControlResponse {
    /// True when the value changed during this frame.
    pub changed: bool,
    /// The widget response, for tooltips and hit testing.
    pub response: egui::Response,
}

impl ControlResponse {
    /// True when the control was clicked this frame.
    ///
    /// # Returns
    ///
    /// `true` for a primary click on the control.
    pub fn clicked(&self) -> bool {
        self.response.clicked()
    }
}

/// Everything a control needs except the value it edits.
struct Spec<'a> {
    /// Caption shown with the control; `""` leaves the dial bare.
    label: &'static str,
    /// How the control is drawn.
    kind: ControlKind,
    /// Inclusive lower bound.
    min: f32,
    /// Inclusive upper bound.
    max: f32,
    /// Unit appended to the read-out.
    suffix: &'static str,
    /// Value restored when the control is reset.
    default: Option<f32>,
    /// Fixed width, when the caller needs one.
    width: Option<f32>,
    /// Automation target, when the control is automatable.
    target: Option<ParameterTarget>,
    /// Called after every accepted change.
    callback: Option<&'a mut dyn FnMut(f32)>,
}

/// A labelled control bound to a value, optionally automatable.
pub struct ValueControl<'a> {
    spec: Spec<'a>,
    bound: Bound<'a>,
}

impl<'a> ValueControl<'a> {
    /// Builds a control around a continuous value.
    ///
    /// # Parameters
    ///
    /// - `label`: text shown with the control.
    /// - `value`: the value to edit, written back in place.
    /// - `range`: inclusive lower and upper bound.
    ///
    /// # Returns
    ///
    /// A control that draws as a slider until another kind is chosen.
    pub fn new(label: &'static str, value: &'a mut f32, range: std::ops::RangeInclusive<f32>) -> Self {
        Self {
            spec: Spec {
                label,
                kind: ControlKind::Slider,
                min: *range.start(),
                max: *range.end(),
                suffix: "",
                default: None,
                width: None,
                target: None,
                callback: None,
            },
            bound: Bound::Number(value),
        }
    }

    /// Builds a rotary knob around a continuous value.
    ///
    /// # Parameters
    ///
    /// - `label`: short caption painted above the dial.
    /// - `value`: the value to edit.
    /// - `range`: inclusive lower and upper bound.
    ///
    /// # Returns
    ///
    /// A [`ControlKind::Knob`] control.
    pub fn knob(label: &'static str, value: &'a mut f32, range: std::ops::RangeInclusive<f32>) -> Self {
        let mut control = Self::new(label, value, range);
        control.spec.kind = ControlKind::Knob;
        control
    }

    /// Builds a numeric box around a continuous value.
    ///
    /// # Parameters
    ///
    /// - `label`: caption shown next to the box.
    /// - `value`: the value to edit.
    /// - `range`: inclusive lower and upper bound.
    ///
    /// # Returns
    ///
    /// A [`ControlKind::Number`] control.
    pub fn number(label: &'static str, value: &'a mut f32, range: std::ops::RangeInclusive<f32>) -> Self {
        let mut control = Self::new(label, value, range);
        control.spec.kind = ControlKind::Number;
        control
    }

    /// Builds a toggle around a flag.
    ///
    /// # Parameters
    ///
    /// - `label`: text beside the box.
    /// - `value`: the flag to edit.
    ///
    /// # Returns
    ///
    /// A [`ControlKind::Toggle`] control; its automation callback reports `1.0`
    /// for on and `0.0` for off.
    pub fn toggle(label: &'static str, value: &'a mut bool) -> Self {
        Self {
            spec: Spec {
                label,
                kind: ControlKind::Toggle,
                min: 0.0,
                max: 1.0,
                suffix: "",
                default: None,
                width: None,
                target: None,
                callback: None,
            },
            bound: Bound::Flag(value),
        }
    }

    /// Builds a drop-down around an enum index.
    ///
    /// # Parameters
    ///
    /// - `label`: caption shown before the drop-down.
    /// - `index`: the selected entry, written back in place.
    /// - `labels`: every entry, in the index order the automation uses.
    ///
    /// # Returns
    ///
    /// A [`ControlKind::Choice`] control.
    pub fn choice(
        label: &'static str,
        index: &'a mut usize,
        labels: &'a [&'a str],
    ) -> Self {
        let max = labels.len().saturating_sub(1) as f32;
        Self {
            spec: Spec {
                label,
                kind: ControlKind::Choice,
                min: 0.0,
                max,
                suffix: "",
                default: None,
                width: None,
                target: None,
                callback: None,
            },
            bound: Bound::Choice { index, labels },
        }
    }

    /// Marks the control as an automation target.
    ///
    /// # Parameters
    ///
    /// - `target`: the parameter this control edits.
    ///
    /// # Returns
    ///
    /// The control, now following the live engine value and offering a lane.
    pub fn automates(mut self, target: ParameterTarget) -> Self {
        self.spec.target = Some(target);
        self
    }

    /// Sets the value a modifier-click restores.
    ///
    /// # Parameters
    ///
    /// - `value`: the default in the control's own units.
    ///
    /// # Returns
    ///
    /// The control, now resettable with Alt+click.
    pub fn with_default(mut self, value: f32) -> Self {
        self.spec.default = Some(value);
        self
    }

    /// Appends a unit to the read-out.
    ///
    /// # Parameters
    ///
    /// - `suffix`: text added after the number, for example `" dB"`.
    ///
    /// # Returns
    ///
    /// The control with that suffix.
    pub fn with_suffix(mut self, suffix: &'static str) -> Self {
        self.spec.suffix = suffix;
        self
    }

    /// Fixes the width of the drawn control.
    ///
    /// # Parameters
    ///
    /// - `width`: width in points.
    ///
    /// # Returns
    ///
    /// The control with that width.
    pub fn with_width(mut self, width: f32) -> Self {
        self.spec.width = Some(width);
        self
    }

    /// Runs a callback whenever the value changes.
    ///
    /// # Parameters
    ///
    /// - `callback`: called with the new value after the binding is updated.
    ///
    /// # Returns
    ///
    /// The control with that callback attached.
    pub fn on_change(mut self, callback: &'a mut dyn FnMut(f32)) -> Self {
        self.spec.callback = Some(callback);
        self
    }

    /// Draws the control.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent layout.
    ///
    /// # Returns
    ///
    /// A [`ControlResponse`] saying whether the value changed and carrying the
    /// widget response for tooltips.
    pub fn show(self, ui: &mut egui::Ui) -> ControlResponse {
        let ValueControl { mut spec, bound } = self;
        let live = spec
            .target
            .and_then(|target| target_live_value(ui, target));
        match bound {
            Bound::Number(value) => show_number(ui, &mut spec, value, live),
            Bound::Flag(flag) => show_flag(ui, &mut spec, flag, live),
            Bound::Choice { index, labels } => show_choice(ui, &mut spec, index, labels, live),
        }
    }
}

/// Draws a knob, slider or number box.
fn show_number(
    ui: &mut egui::Ui,
    spec: &mut Spec<'_>,
    value: &mut f32,
    live: Option<f32>,
) -> ControlResponse {
    // While a lane drives the value the control shows what is heard; the
    // binding is only written when the user edits it.
    let mut shown = live.unwrap_or(*value);
    let (response, changed) = match spec.kind {
        ControlKind::Knob => {
            let mut knob = Knob::new(spec.label, spec.min, spec.max, spec.suffix);
            if let Some(default) = spec.default {
                knob = knob.with_default(default);
            }
            if let Some(target) = spec.target {
                knob = knob.automates(target);
            }
            let (changed, response) = knob.show_response(ui, &mut shown);
            (response, changed)
        }
        ControlKind::Number => {
            let widget = egui::DragValue::new(&mut shown)
                .range(spec.min..=spec.max)
                .speed((spec.max - spec.min) / 200.0)
                .suffix(spec.suffix);
            let response = match spec.width {
                Some(width) => ui.add_sized([width, ui.spacing().interact_size.y], widget),
                None => ui.add(widget),
            };
            let changed = response.changed();
            (response, changed)
        }
        _ => {
            let widget = if spec.label.is_empty() {
                egui::Slider::new(&mut shown, spec.min..=spec.max)
            } else {
                egui::Slider::new(&mut shown, spec.min..=spec.max).text(spec.label)
            };
            let response = match spec.width {
                Some(width) => ui.add_sized([width, ui.spacing().interact_size.y], widget),
                None => ui.add(widget),
            };
            let changed = response.changed();
            (response, changed)
        }
    };
    if changed {
        *value = shown.clamp(spec.min, spec.max);
        if let Some(callback) = spec.callback.as_mut() {
            callback(*value);
        }
    }
    // The knob draws the automated ring and its own menu.
    if let Some(target) = spec.target
        && spec.kind != ControlKind::Knob
    {
        automation_context_menu(&response, &format!("{} · {shown:.2}", spec.label), target);
    }
    ControlResponse { changed, response }
}

/// Draws a flag.
fn show_flag(
    ui: &mut egui::Ui,
    spec: &mut Spec<'_>,
    flag: &mut bool,
    live: Option<f32>,
) -> ControlResponse {
    let mut shown = live.map_or(*flag, |value| value >= 0.5);
    let response = ui.checkbox(&mut shown, spec.label);
    let changed = response.changed();
    if changed {
        *flag = shown;
        if let Some(callback) = spec.callback.as_mut() {
            callback(if shown { 1.0 } else { 0.0 });
        }
    }
    if let Some(target) = spec.target {
        automation_context_menu(&response, spec.label, target);
    }
    ControlResponse { changed, response }
}

/// Draws an enum drop-down.
fn show_choice(
    ui: &mut egui::Ui,
    spec: &mut Spec<'_>,
    index: &mut usize,
    labels: &[&str],
    live: Option<f32>,
) -> ControlResponse {
    let shown_index = live
        .map(|value| (value.round().max(0.0) as usize).min(labels.len().saturating_sub(1)))
        .unwrap_or(*index);
    let selected = labels.get(shown_index).copied().unwrap_or("");
    let mut changed = false;
    let response = ui
        .horizontal(|ui| {
            if !spec.label.is_empty() {
                ui.label(spec.label);
            }
            let combo = egui::ComboBox::from_id_salt(("value_control", spec.label))
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (position, entry) in labels.iter().enumerate() {
                        if ui.selectable_value(index, position, *entry).changed() {
                            changed = true;
                        }
                    }
                })
                .response;
            if let Some(target) = spec.target {
                // A combo box owns a popup of its own, so the lane is requested
                // from a button beside it.
                automation_lane_button(ui, spec.label, target);
            }
            combo
        })
        .inner;
    if changed {
        if let Some(callback) = spec.callback.as_mut() {
            callback(*index as f32);
        }
    }
    ControlResponse { changed, response }
}

/// Creates a lane for `target` without drawing a control.
///
/// # Parameters
///
/// - `context`: the egui context the request is queued in.
/// - `target`: the parameter to automate.
pub fn automate(context: &egui::Context, target: ParameterTarget) {
    request_automation_lane(context, target);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_bound_to_a_value_reports_the_range() {
        let mut value = 3.0;
        let control = ValueControl::slider_demo(&mut value);
        assert_eq!(control.spec.min, 0.0);
        assert_eq!(control.spec.max, 10.0);
        assert_eq!(control.spec.kind, ControlKind::Slider);
    }

    #[test]
    fn a_choice_control_spans_its_labels() {
        let mut index = 1;
        let labels = ["Sine", "Saw", "Square"];
        let control = ValueControl::choice("Wave", &mut index, &labels);
        assert_eq!(control.spec.min, 0.0);
        assert_eq!(control.spec.max, 2.0);
        assert!(matches!(control.bound, Bound::Choice { .. }));
    }
}

#[cfg(test)]
impl<'a> ValueControl<'a> {
    /// Test-only helper: a slider control over a caller-provided value.
    fn slider_demo(value: &'a mut f32) -> Self {
        Self::new("Demo", value, 0.0..=10.0)
    }
}
