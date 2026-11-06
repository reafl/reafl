//! Creating automation lanes for new targets.

use crate::*;

impl PulseForgeApp {
    /// Adds an automation lane for any target, styling it from the current
    /// value so the first point matches what the user sees.
    pub(crate) fn create_automation_for_target(&mut self, target: ParameterTarget) {
        self.push_history();
        let label = parameter_target_label(&self.project, target);
        let base = self
            .project
            .parameter_value(target)
            .map_or(0.5, |value| target.normalize_value(value));
        let track_id = self.project.add_automation_track(AutomationTrack {
            name: label,
            target,
            points: vec![AutomationPoint {
                step: 0,
                value: base,
                curve: AutomationCurve::Linear,
            }],
            ..AutomationTrack::default()
        });
        self.status = format!("Automation lane created for track {}.", track_id + 1);
        // Show the new lane straight away, so the action has a visible result.
        self.windows.automation = true;
        self.sync_project();
    }

    /// Creates an automation lane that drives a hosted plug-in parameter.
    pub(crate) fn create_plugin_automation(&mut self, host_id: u64, name: &str) {
        let parameter_id = self
            .hosted_meta
            .get(&host_id)
            .and_then(|meta| meta.parameters.iter().find(|p| p.name == name))
            .map(|parameter| parameter.id)
            .unwrap_or(0);
        self.push_history();
        let track_id = self.project.add_automation_track(AutomationTrack {
            name: name.to_owned(),
            target: ParameterTarget::PluginParameter {
                host_id,
                parameter_id,
            },
            ..AutomationTrack::default()
        });
        if let Some(track) = self.project.automation_tracks.get_mut(track_id) {
            track.name = name.to_owned();
        }
        self.status = format!("Automation lane created for {name}.");
        self.sync_project();
    }
}
