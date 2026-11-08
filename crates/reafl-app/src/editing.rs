//! Editing helpers that only touch project state.

use crate::*;

impl ReaflApp {
    /// Moves the global snap one entry up or down the fixed step list.
    pub(crate) fn step_snap(&mut self, direction: i32) {
        let current = self
            .project
            .snap_ticks
            .clamp(SNAP_STEPS[0], *SNAP_STEPS.last().unwrap_or(&128));
        let index = SNAP_STEPS
            .iter()
            .position(|value| *value >= current)
            .unwrap_or(0);
        let next = if direction > 0 {
            (index + 1).min(SNAP_STEPS.len() - 1)
        } else {
            index.saturating_sub(1)
        };
        self.project.snap_ticks = SNAP_STEPS[next];
        self.status = format!("Snap: {}", snap_label(SNAP_STEPS[next]));
        self.sync_project();
    }

    /// Accent colour from the theming page.
    pub(crate) fn ui_accent(&self) -> egui::Color32 {
        egui::Color32::from_rgb(
            self.settings.accent[0],
            self.settings.accent[1],
            self.settings.accent[2],
        )
    }

    /// Applies the current repeat settings to the selected note, or to every
    /// note at the selection's pitch when a note is selected.
    pub(crate) fn apply_repeat_to_selection(&mut self) {
        let Some(Selection::Note {
            track,
            clip,
            pitch,
            start_step,
        }) = self.selection
        else {
            self.status = "Select a note in the piano roll first.".to_owned();
            return;
        };
        let count = self.note_repeat_count;
        let ticks = self.note_repeat_ticks;
        let decay = self.note_repeat_decay;
        let Some(clip) = self
            .project
            .tracks
            .get_mut(track)
            .and_then(|track| track.clips.get_mut(clip))
        else {
            return;
        };
        let mut updated = 0;
        for note in &mut clip.notes {
            if note.pitch == pitch && note.start_step == start_step {
                note.repeat_count = count;
                note.repeat_ticks = ticks;
                note.repeat_decay = decay;
                updated += 1;
            }
        }
        if updated == 0 {
            self.status = "That note no longer exists.".to_owned();
            return;
        }
        self.sync_project();
        self.status = format!("{updated} note(s) updated with {count} repeat(s).");
    }

    /// Opens the rename prompt for a track. The mixer channel shares the name,
    /// so one action renames both.
    pub(crate) fn begin_rename_track(&mut self) {
        let lane = self.lane_for_selection();
        self.rename_buffer = self
            .project
            .tracks
            .get(lane)
            .map(|lane| lane.name.clone())
            .unwrap_or_default();
        self.renaming_track = Some(lane);
    }

    /// Track that rename/reorder actions target: the selected clip's lane, else
    /// the selected mixer channel.
    pub(crate) fn lane_for_selection(&self) -> usize {
        match self.selection {
            Some(Selection::Clip { track: lane, .. }) => lane.min(self.project.tracks.len() - 1),
            _ => self.selected_lane.min(self.project.tracks.len() - 1),
        }
    }

    pub(crate) fn commit_rename_track(&mut self) {
        let Some(lane) = self.renaming_track.take() else {
            return;
        };
        let name = self.rename_buffer.trim().to_owned();
        if name.is_empty() {
            self.status = "Track name cannot be empty.".to_owned();
            return;
        }
        if let Some(target) = self.project.tracks.get_mut(lane) {
            target.name = name.clone();
        }
        self.push_history();
        self.status = format!("Renamed track to {name} (mixer channel renamed too).");
        self.sync_project();
    }

    /// Selects an editing tool, remembering that the arrangement has no Join.
    pub(crate) fn set_tool(&mut self, tool: PianoTool) {
        self.piano_tool = tool;
        // The hotkeys are shared: pressing S must select in the arrangement too,
        // not just in the piano roll. Join exists only in the piano roll, so the
        // arrangement keeps whatever tool it had.
        if tool != PianoTool::Join {
            self.arrangement_tool = tool;
        }
        let name = match tool {
            PianoTool::Pen => "Pen",
            PianoTool::Select => "Select",
            PianoTool::Brush => "Brush",
            PianoTool::Join => "Join (piano roll only)",
            PianoTool::Cut => "Cut",
        };
        self.status = format!("{name} tool.");
    }

    pub(crate) fn effect_slot_name(&self, channel: usize, slot: usize) -> Option<String> {
        let slots = if channel == self.project.tracks.len() {
            &self.project.master.inserts
        } else {
            &self.project.tracks.get(channel)?.inserts
        };
        let name = &slots.get(slot)?.name;
        (!name.is_empty()).then(|| name.clone())
    }

    pub(crate) fn effect_slot_mut(
        &mut self,
        channel: usize,
        slot: usize,
    ) -> Option<&mut InsertFxSlot> {
        let slots = if channel == self.project.tracks.len() {
            &mut self.project.master.inserts
        } else {
            &mut self.project.tracks.get_mut(channel)?.inserts
        };
        slots.get_mut(slot)
    }
}
