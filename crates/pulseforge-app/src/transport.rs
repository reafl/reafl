//! Project syncing, history, undo and the playhead.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn sender(&self) -> Option<SyncSender<AudioCommand>> {
        self.audio.as_ref().map(|audio| audio.sender.clone())
    }

    pub(crate) fn send_audio(&mut self, command: AudioCommand) -> bool {
        let Some(sender) = self.sender() else {
            self.status = self
                .audio_error
                .clone()
                .unwrap_or_else(|| "Audio output is unavailable.".to_owned());
            return false;
        };
        match sender.try_send(command) {
            Ok(()) => true,
            Err(error) => {
                self.status = format!("Audio update failed: {error}");
                false
            }
        }
    }

    pub(crate) fn sync_project(&mut self) {
        self.project.normalize();
        let routes = match self.midi_routes.lock() {
            Ok(mut routes) => {
                *routes = self.project.midi_routes.to_vec();
                true
            }
            Err(_) => false,
        };
        if !routes {
            self.status = "MIDI routing table could not be updated.".to_owned();
        }
        self.project_dirty = true;
        self.flush_project();
    }

    /// Coalesces project snapshots so continuous edits (knob drags, painting)
    /// cannot flood the audio thread with full project rebuilds.
    pub(crate) fn flush_project(&mut self) {
        const MIN_SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);
        if !self.project_dirty || self.last_project_sync.elapsed() < MIN_SYNC_INTERVAL {
            return;
        }
        self.project_dirty = false;
        self.last_project_sync = std::time::Instant::now();
        if let Some(audio) = &self.audio {
            if let Err(error) = audio.set_project(self.project.clone()) {
                self.status = format!("Could not queue project update: {error}");
            }
        } else {
            self.send_audio(AudioCommand::SetProject(Box::new(self.project.clone())));
        }
    }

    /// Snapshots the project so the next edit can be undone. Callers use this
    /// at the start of a gesture or discrete action, never per frame.
    pub(crate) fn push_history(&mut self) {
        const HISTORY_LIMIT: usize = 64;
        self.history.push(self.project.clone());
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
        self.redo_history.clear();
    }

    pub(crate) fn undo(&mut self) {
        let Some(previous) = self.history.pop() else {
            self.status = "Nothing to undo.".to_owned();
            return;
        };
        self.redo_history
            .push(std::mem::replace(&mut self.project, previous));
        self.project.normalize();
        self.sync_project();
        self.status = "Undo.".to_owned();
    }

    pub(crate) fn redo(&mut self) {
        let Some(next) = self.redo_history.pop() else {
            self.status = "Nothing to redo.".to_owned();
            return;
        };
        self.history
            .push(std::mem::replace(&mut self.project, next));
        self.project.normalize();
        self.sync_project();
        self.status = "Redo.".to_owned();
    }

    /// Advances the displayed playhead using the audio thread's published
    /// position plus a short extrapolation, so motion is frame-rate smooth
    /// instead of stepping once per audio buffer.
    pub(crate) fn advance_playhead(&mut self) {
        let Some(audio) = &self.audio else {
            self.playhead_display = 0.0;
            return;
        };
        let raw = audio
            .playhead_position
            .load(std::sync::atomic::Ordering::Relaxed);
        if raw != self.playhead_raw {
            self.playhead_raw = raw;
            self.playhead_anchor = std::time::Instant::now();
        }
        let base = f64::from_bits(raw);
        if !self.playing {
            self.playhead_display = base;
            return;
        }
        let steps_per_second = f64::from(self.project.bpm) / 60.0 * 4.0;
        let elapsed = self.playhead_anchor.elapsed().as_secs_f64().min(0.12);
        let predicted = base + elapsed * steps_per_second;
        let delta = predicted - self.playhead_display;
        if delta.abs() > 2.0 || delta < -0.05 {
            // Transport jump (loop wrap or seek): follow it immediately.
            self.playhead_display = predicted;
        } else {
            self.playhead_display += delta * 0.4;
        }
    }

    /// Playhead position in steps, wrapped when previewing a single pattern.
    pub(crate) fn playhead_step(&self) -> f64 {
        let position = self.playhead_display.max(0.0);
        if self.selected_clip_playback {
            let length = self
                .project
                .patterns
                .get(self.selected_clip)
                .map_or(1, |pattern| pattern.length_steps.max(1));
            position % length as f64
        } else if self.playing
            && self.loop_enabled
            && let Some((start, end)) = self.arrangement_loop
            && end > start
            && position >= end as f64
        {
            let span = (end - start) as f64;
            start as f64 + (position - end as f64) % span
        } else {
            position
        }
    }
}
