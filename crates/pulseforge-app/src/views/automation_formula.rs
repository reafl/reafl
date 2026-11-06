//! The formula tool of the automation window.

use crate::*;

impl PulseForgeApp {
    /// Shows the formula editor, a live preview and the button that samples
    /// the expression into a lane.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI.
    pub(crate) fn automation_formula_tool(&mut self, ui: &mut egui::Ui) {
        // Quick formula tool: an expression is sampled into a real curve lane,
        // so the engine needs no scripting path of its own.
        ui.horizontal(|ui| {
            ui.label("Formula");
            ui.add(
                egui::TextEdit::singleline(&mut self.formula_text)
                    .desired_width(260.0)
                    .hint_text("0.5 + 0.4 * sin(2 * pi * x)"),
            );
        });
        match pulseforge_core::expr::Expression::parse(&self.formula_text) {
            Ok(expression) => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "x=0.25 \u{2192} {:.3} \u{00B7} x=0.5 \u{2192} {:.3} \u{00B7} x=0.75 \u{2192} {:.3}",
                            expression.evaluate(0.25),
                            expression.evaluate(0.5),
                            expression.evaluate(0.75)
                        ))
                        .small()
                        .weak(),
                    );
                    if ui.button("Add formula lane").clicked() {
                        self.add_formula_automation(&expression);
                    }
                });
            }
            Err(error) => {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("formula: {error}"));
            }
        }
    }
}

impl PulseForgeApp {
    /// Samples a parsed formula into a curve lane on the current automation
    /// target, so a formula behaves like any hand-drawn lane afterwards.
    pub(crate) fn add_formula_automation(
        &mut self,
        expression: &pulseforge_core::expr::Expression,
    ) {
        const LENGTH_STEPS: u64 = 64;
        const POINT_COUNT: u64 = 32;
        let Some(target) = self.selected_automation_target() else {
            self.status = "Pick an automatable parameter first.".to_owned();
            return;
        };
        let points: Vec<AutomationPoint> = (0..=POINT_COUNT)
            .map(|index| AutomationPoint {
                step: LENGTH_STEPS * index / POINT_COUNT,
                value: expression
                    .evaluate(index as f64 / POINT_COUNT as f64)
                    .clamp(0.0, 1.0) as f32,
                curve: AutomationCurve::Linear,
            })
            .collect();
        let label = parameter_target_label(&self.project, target);
        self.push_history();
        self.project.automation_tracks.push(AutomationTrack {
            name: format!("{label} (formula)"),
            target,
            start_step: 0,
            length_steps: LENGTH_STEPS,
            points,
            modulation_points: Vec::new(),
            enabled: true,
            source: project::AutomationSource::Curve,
        });
        self.sync_project();
        self.status = "Formula sampled into a new automation lane.".to_owned();
    }
}
