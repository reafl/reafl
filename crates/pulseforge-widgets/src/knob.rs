//! The rotary knob and its automation menu.

use eframe::egui;
use pulseforge_core::project::ParameterTarget;

use crate::automation_bridge::{target_is_automated, target_live_value};
use crate::platform;

/// A rotary knob bound to a value, optionally following an automation lane.
pub struct Knob {
    /// Short caption painted above the dial; `""` leaves it bare.
    pub label: &'static str,
    /// Value at the bottom of the dial's travel.
    pub min: f32,
    /// Value at the top of the dial's travel.
    pub max: f32,
    /// Unit appended to the read-out, for example `" dB"`.
    pub suffix: &'static str,
    /// Dial diameter in points; anything under 40 draws the compact form.
    pub size: f32,
    /// Value restored by Alt+click and by the menu's reset entry.
    pub default: Option<f32>,
    /// Parameter this knob drives, when it is automatable.
    pub automation_target: Option<ParameterTarget>,
}

impl Knob {
    /// Creates a knob for a value range.
    ///
    /// # Parameters
    ///
    /// - `label`: caption above the dial.
    /// - `min`: value at the bottom of the travel.
    /// - `max`: value at the top of the travel.
    /// - `suffix`: unit appended to the read-out.
    ///
    /// # Returns
    ///
    /// A full-size knob with no automation target.
    pub fn new(label: &'static str, min: f32, max: f32, suffix: &'static str) -> Self {
        Self {
            label,
            min,
            max,
            suffix,
            size: 58.0,
            default: None,
            automation_target: None,
        }
    }

    /// Sets the value Alt+click and the menu's reset entry restore.
    ///
    /// # Parameters
    ///
    /// - `default`: the value in the knob's own units.
    ///
    /// # Returns
    ///
    /// The knob with that default.
    pub fn with_default(mut self, default: f32) -> Self {
        self.default = Some(default);
        self
    }

    /// Marks the knob as an automation target.
    ///
    /// # Parameters
    ///
    /// - `target`: the parameter the knob edits.
    ///
    /// # Returns
    ///
    /// The knob, now drawing the automated ring and following the live value.
    pub fn automates(mut self, target: ParameterTarget) -> Self {
        self.automation_target = Some(target);
        self
    }

    /// Compact knobs are used inside dense rows such as the channel rack.
    pub fn compact(mut self) -> Self {
        self.size = 22.0;
        self
    }

    /// Draws the knob.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent layout.
    /// - `value`: the value to edit, written back in place.
    ///
    /// # Returns
    ///
    /// `true` when the value changed this frame.
    pub fn show(self, ui: &mut egui::Ui, value: &mut f32) -> bool {
        self.show_response(ui, value).0
    }

    /// Draws the knob and also returns its widget response.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent layout.
    /// - `value`: the value to edit, written back in place.
    ///
    /// # Returns
    ///
    /// Whether the value changed, and the response of the dial.
    pub fn show_response(self, ui: &mut egui::Ui, value: &mut f32) -> (bool, egui::Response) {
        let compact = self.size < 40.0;
        let height = if compact { 40.0 } else { 76.0 };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(self.size, height), egui::Sense::click_and_drag());
        let mut changed = false;
        // A control driven by an automation lane follows the engine value and
        // stops accepting edits, so what is drawn matches what is heard.
        let automated = self
            .automation_target
            .is_some_and(|target| target_is_automated(ui, target));
        let live_value = self
            .automation_target
            .and_then(|target| target_live_value(ui, target));
        // A knob drag is relative: hide the pointer while it runs and put it
        // back where it started so the value does not jump on release.
        let drag_id = response.id.with("knob_drag_origin");
        if response.drag_started() {
            ui.data_mut(|data| data.insert_temp(drag_id, platform::cursor_position()));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
        }
        if response.drag_stopped() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
            if let Some(Some((x, y))) = ui.data_mut(|data| data.remove_temp(drag_id)) {
                platform::set_cursor_position(x, y);
            }
        }
        if response.dragged() {
            let delta = ui.input(|input| input.pointer.delta().y);
            *value = (*value - delta * (self.max - self.min) / 140.0).clamp(self.min, self.max);
            changed = true;
        }
        let (alt, control) = ui.input(|input| (input.modifiers.alt, input.modifiers.ctrl));
        if response.clicked_by(egui::PointerButton::Primary) && alt {
            *value = self.default.unwrap_or((self.min + self.max) * 0.5);
            changed = true;
        }
        if response.clicked_by(egui::PointerButton::Primary) && control {
            ui.data_mut(|data| data.insert_temp(response.id.with("numeric_entry"), true));
        }
        let painter = ui.painter_at(rect);
        let center = egui::pos2(
            rect.center().x,
            rect.top() + if compact { 14.0 } else { 21.0 },
        );
        let radius = if compact { 9.5 } else { 15.0 };
        painter.circle_filled(center, radius + 3.0, egui::Color32::from_gray(18));
        painter.circle_filled(center, radius, egui::Color32::from_gray(51));
        painter.circle_stroke(
            center,
            radius,
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(115)),
        );

        if automated {
            painter.circle_stroke(
                center,
                radius + 5.0,
                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(202, 158, 238)),
            );
        }

        let normalized =
            ((live_value.unwrap_or(*value) - self.min) / (self.max - self.min)).clamp(0.0, 1.0);
        let angle = (-135.0 + normalized * 270.0).to_radians();
        let indicator = center + egui::vec2(angle.sin(), -angle.cos()) * (radius - 3.0);
        painter.line_segment(
            [center, indicator],
            egui::Stroke::new(2.2_f32, egui::Color32::from_rgb(112, 208, 190)),
        );
        if let Some(live) = live_value {
            let live_normalized = ((live - self.min) / (self.max - self.min)).clamp(0.0, 1.0);
            let live_angle = (-135.0 + live_normalized * 270.0).to_radians();
            let live_indicator =
                center + egui::vec2(live_angle.sin(), -live_angle.cos()) * (radius + 1.0);
            painter.circle_filled(live_indicator, 2.6, egui::Color32::from_rgb(238, 205, 116));
        }
        let mut arc_points = Vec::with_capacity(19);
        for index in 0..=18 {
            let arc_angle = (-135.0 + index as f32 * 15.0).to_radians();
            arc_points
                .push(center + egui::vec2(arc_angle.sin(), -arc_angle.cos()) * (radius + 2.0));
        }
        painter.add(egui::Shape::line(
            arc_points,
            egui::Stroke::new(1.3_f32, egui::Color32::from_gray(88)),
        ));
        let label_y = if compact {
            rect.top() + 5.0
        } else {
            rect.top() + 45.0
        };
        // An empty label keeps the dial bare, which is what the mixer strips
        // want: the pan knob is identified by position, not by text.
        if !self.label.is_empty() {
            painter.text(
                egui::pos2(rect.center().x, label_y),
                egui::Align2::CENTER_CENTER,
                self.label,
                egui::FontId::proportional(if compact { 8.0 } else { 10.0 }),
                egui::Color32::from_gray(175),
            );
        }
        let shown = live_value.unwrap_or(*value);
        // Compact knobs omit the value text: their label sits above the dial and
        // the value stays available from the status bar and context menu.
        if !compact {
            painter.text(
                egui::pos2(rect.center().x, rect.top() + 62.0),
                egui::Align2::CENTER_CENTER,
                format!("{shown:.1}{}", self.suffix),
                egui::FontId::monospace(10.0),
                if live_value.is_some() {
                    egui::Color32::from_rgb(238, 205, 116)
                } else {
                    egui::Color32::WHITE
                },
            );
        }
        let numeric_id = response.id.with("numeric_entry");
        let mut editing = ui
            .data(|data| data.get_temp::<bool>(numeric_id))
            .unwrap_or(false);
        if editing {
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .range(self.min..=self.max)
                        .speed((self.max - self.min) / 100.0)
                        .suffix(self.suffix),
                )
                .changed();
            if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                ui.data_mut(|data| data.remove::<bool>(numeric_id));
                editing = false;
            }
        }
        response.context_menu(|ui| {
            ui.label(format!("{} value", self.label));
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .range(self.min..=self.max)
                        .speed((self.max - self.min) / 100.0)
                        .suffix(self.suffix),
                )
                .changed();
            if ui
                .add_enabled(
                    self.automation_target.is_some(),
                    egui::Button::new("Add automation lane..."),
                )
                .clicked()
            {
                if let Some(target) = self.automation_target {
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new("knob_add_automation"), target)
                    });
                    ui.close();
                }
            }
            if ui.button("Reset to default").clicked() {
                *value = self.default.unwrap_or((self.min + self.max) * 0.5);
                changed = true;
                ui.close();
            }
            ui.separator();
            ui.add_enabled(false, egui::Button::new("More parameter actions"));
        });
        response.clone().on_hover_text(format!(
            "{}: {:.2}{} · Alt+click reset · Ctrl+click type · right-click menu",
            self.label, *value, self.suffix
        ));
        let _ = editing;
        (changed, response)
    }
}
