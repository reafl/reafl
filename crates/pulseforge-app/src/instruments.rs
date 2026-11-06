//! Instrument naming, colours and power switches.

use crate::*;

impl PulseForgeApp {
    /// Colour of a channel row: its custom colour or the generated one.
    pub(crate) fn instrument_color(&self, instrument_id: usize) -> egui::Color32 {
        self.project
            .instruments
            .get(instrument_id)
            .and_then(|instrument| instrument.color)
            .map_or_else(
                || lane_color(instrument_id),
                |[r, g, b]| egui::Color32::from_rgb(r, g, b),
            )
    }

    /// Commits the inline instrument rename started from the channel menu.
    pub(crate) fn commit_instrument_rename(&mut self) {
        let Some(instrument_id) = self.renaming_instrument.take() else {
            return;
        };
        let name = self.rename_buffer.trim().to_owned();
        if name.is_empty() {
            return;
        }
        if let Some(instrument) = self.project.instruments.get_mut(instrument_id) {
            instrument.name = name;
        }
        self.sync_project();
    }

    /// Number of arrangement steps a sample spans at the current tempo.
    pub(crate) fn steps_for_sample(&self, sample: &audio::SampleData) -> u64 {
        let seconds = sample.frames.len() as f64 / f64::from(sample.sample_rate.max(1));
        let steps_per_second = f64::from(self.project.bpm) / 60.0 * 4.0;
        (seconds * steps_per_second).round().max(1.0) as u64
    }

    /// Same semantics for an instrument's power switch.
    pub(crate) fn toggle_instrument_switch(&mut self, instrument: usize) {
        let (shift, ctrl) = (self.mod_shift, self.mod_ctrl);
        let Some(item) = self.project.instruments.get_mut(instrument) else {
            return;
        };
        if shift {
            item.locked = !item.locked;
            self.status = if item.locked {
                format!("{} locked.", item.name)
            } else {
                format!("{} unlocked.", item.name)
            };
        } else if ctrl {
            item.solo = !item.solo;
            self.status = if item.solo {
                format!("{} soloed.", item.name)
            } else {
                format!("{} unsoloed.", item.name)
            };
        } else {
            // See `toggle_lane_switch`: a lock protects against global changes,
            // never against the user's own click.
            item.enabled = !item.enabled;
            self.status = if item.locked {
                format!("{} {} (locked).", item.name, power_word(item.enabled))
            } else {
                format!("{} {}.", item.name, power_word(item.enabled))
            };
        }
        self.push_history();
        self.sync_project();
    }
}
