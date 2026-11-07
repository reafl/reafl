//! Track list edits, clip edits and the insert/equaliser normalisers.

use super::*;

impl Project {
    /// Appends a new source track and returns its index.
    ///
    /// # Returns
    ///
    /// The index of the new track.
    pub fn add_track(&mut self) -> TrackId {
        let id = self.tracks.len();
        self.tracks.push(Track::source(
            format!("Track {:02}", id + 1),
            Generator::default(),
        ));
        self.normalize();
        id
    }

    /// Appends a new folder track and returns its index.
    ///
    /// # Returns
    ///
    /// The index of the new group.
    pub fn add_group(&mut self) -> TrackId {
        let id = self.tracks.len();
        self.tracks.push(Track::group(format!("Folder {:02}", id + 1)));
        self.normalize();
        id
    }

    /// Wraps a track in a new folder, so it becomes the folder's only child.
    ///
    /// # Parameters
    ///
    /// - `track`: the track to group.
    ///
    /// # Returns
    ///
    /// The new folder's index, or `None` when the track does not exist.
    pub fn group_track(&mut self, track: TrackId) -> Option<TrackId> {
        if track >= self.tracks.len() {
            return None;
        }
        let group = self.tracks.len();
        let parent = self.tracks[track].parent;
        let mut folder = Track::group(format!("Folder {:02}", group + 1));
        folder.parent = parent;
        self.tracks.push(folder);
        self.tracks[track].parent = Some(group);
        self.normalize();
        Some(group)
    }

    /// Moves a track into a folder, or back to the top level.
    ///
    /// # Parameters
    ///
    /// - `track`: the track to move.
    /// - `parent`: the new folder, or `None` for the top level.
    ///
    /// # Returns
    ///
    /// `false` when the move would create a cycle or target a source track.
    pub fn reparent_track(&mut self, track: TrackId, parent: Option<TrackId>) -> bool {
        if !Track::can_become_child_of(&self.tracks, track, parent) {
            return false;
        }
        if let Some(entry) = self.tracks.get_mut(track) {
            entry.parent = parent;
        }
        self.normalize();
        true
    }

    /// Moves track `from` to position `to`, keeping mixer sends, routing and
    /// insert automation pointing at the same tracks.
    ///
    /// # Parameters
    ///
    /// - `from`: current index of the track.
    /// - `to`: index it should end up at.
    pub fn move_track(&mut self, from: usize, to: usize) {
        let track_count = self.tracks.len();
        if from >= track_count || to >= track_count || from == to {
            return;
        }
        let master_index = track_count;
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        // Permute the send matrix the same way, keeping the master row/column
        // last so audio routing still matches the new index order.
        if self.sends.len() == track_count + 1
            && self.sends.iter().all(|row| row.len() == track_count + 1)
        {
            let mut rows: Vec<Vec<f32>> = self.sends[..track_count].to_vec();
            let row = rows.remove(from);
            rows.insert(to, row);
            let mut permuted = vec![vec![0.0; track_count + 1]; track_count + 1];
            for (new_row, source_row) in rows.iter().enumerate() {
                let mut values = source_row[..track_count].to_vec();
                let value = values.remove(from);
                values.insert(to, value);
                permuted[new_row][..track_count].copy_from_slice(&values);
                permuted[new_row][master_index] = source_row[master_index];
            }
            let master_row = self.sends[master_index].clone();
            let mut values = master_row[..track_count].to_vec();
            let value = values.remove(from);
            values.insert(to, value);
            permuted[master_index][..track_count].copy_from_slice(&values);
            self.sends = permuted;
        }
        let remap = |index: usize| -> usize {
            if index == master_index {
                master_index
            } else if index == from {
                to
            } else if from < to && index > from && index <= to {
                index - 1
            } else if to < from && index >= to && index < from {
                index + 1
            } else {
                index
            }
        };
        for track in &mut self.tracks {
            if let Some(parent) = &mut track.parent {
                *parent = remap(*parent);
            }
        }
        for track in &mut self.automation_tracks {
            match &mut track.target {
                ParameterTarget::Track { track_id, .. }
                | ParameterTarget::InsertEffect { track_id, .. }
                | ParameterTarget::Clip { track_id, .. } => {
                    *track_id = remap((*track_id).min(master_index));
                }
                _ => {}
            }
        }
        self.normalize();
    }

    /// Reorders an automation lane so grouped rows can be rearranged.
    ///
    /// # Parameters
    ///
    /// - `from`: current index of the lane.
    /// - `to`: index it should end up at.
    pub fn move_automation(&mut self, from: usize, to: usize) {
        if from >= self.automation_tracks.len() || to >= self.automation_tracks.len() || from == to
        {
            return;
        }
        let track = self.automation_tracks.remove(from);
        self.automation_tracks.insert(to, track);
    }

    /// Appends an automation lane and returns its index.
    ///
    /// # Parameters
    ///
    /// - `track`: the lane to add.
    pub fn add_automation_track(&mut self, track: AutomationTrack) -> usize {
        self.automation_tracks.push(track);
        self.automation_tracks.len() - 1
    }

    /// Removes a track, its children, its automation and its routing.
    ///
    /// # Parameters
    ///
    /// - `track`: the track to remove.
    ///
    /// # Returns
    ///
    /// The number of tracks removed, which is more than one for a folder.
    pub fn remove_track(&mut self, track: TrackId) -> usize {
        if track >= self.tracks.len() {
            return 0;
        }
        // Children first, deepest last, so index shifts stay valid.
        let mut victims: Vec<TrackId> = Track::descendants(&self.tracks, track);
        victims.push(track);
        victims.sort_unstable_by(|left, right| right.cmp(left));
        let removed = victims.len();
        for victim in victims {
            self.tracks.remove(victim);
            if let Some(row) = self.sends.get_mut(victim) {
                row.fill(0.0);
            }
            for row in self.sends.iter_mut() {
                if let Some(gain) = row.get_mut(victim) {
                    *gain = 0.0;
                }
            }
            for index in 0..self.automation_tracks.len() {
                let target = self.automation_tracks[index].target;
                if target_track(target) == Some(victim) {
                    self.automation_tracks.remove(index);
                    break;
                }
            }
            for entry in &mut self.tracks {
                entry.parent = match entry.parent {
                    Some(parent) if parent > victim => Some(parent - 1),
                    Some(parent) if parent == victim => None,
                    other => other,
                };
            }
            for entry in &mut self.automation_tracks {
                match &mut entry.target {
                    ParameterTarget::Track { track_id, .. }
                    | ParameterTarget::InsertEffect { track_id, .. }
                    | ParameterTarget::Clip { track_id, .. } => {
                        if *track_id > victim {
                            *track_id -= 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        self.normalize();
        removed
    }

    /// Copies a clip onto the same track, immediately after it.
    ///
    /// # Parameters
    ///
    /// - `track_id`: track owning the clip.
    /// - `clip_index`: index of the clip inside the track.
    ///
    /// # Returns
    ///
    /// The index of the new clip, or `None` when the source clip is missing.
    pub fn duplicate_clip_after(&mut self, track_id: TrackId, clip_index: usize) -> Option<usize> {
        let track = self.tracks.get_mut(track_id)?;
        let source = track.clips.get(clip_index)?.clone();
        let start_step = source.start_step.saturating_add(source.length_steps.max(1));
        let insertion_index = track
            .clips
            .iter()
            .take_while(|clip| clip.start_step <= start_step)
            .count();
        track.clips.insert(
            insertion_index,
            Clip {
                start_step,
                offset_steps: 0,
                ..source
            },
        );
        Some(insertion_index)
    }

    /// The notes a track plays at a transport position, after its MIDI FX.
    ///
    /// # Parameters
    ///
    /// - `track_id`: track to read.
    /// - `step`: transport position in steps.
    ///
    /// # Returns
    ///
    /// The notes whose trigger offset lands exactly on `step`, already
    /// transposed and scaled. A group has no notes of its own: its clips are
    /// forwarded by the engine to the descendants.
    pub fn notes_at(&self, track_id: TrackId, step: u64) -> Vec<ClipNote> {
        let Some(track) = self.tracks.get(track_id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for clip in track.clips_at(step) {
            let offset = step - clip.start_step;
            for note in &clip.notes {
                let note_start = note.start_tick() / TICKS_PER_STEP;
                if !note.trigger_offsets().iter().any(|(tick, _)| {
                    note_start + tick / TICKS_PER_STEP + clip.offset_steps == offset
                }) {
                    continue;
                }
                out.push(track.midi_fx.process(*note));
            }
        }
        out
    }

    pub(crate) fn normalize_eq(eq: &mut ThreeBandEq) {
        eq.low_db = finite_clamp(eq.low_db, -12.0, 12.0, 0.0);
        eq.mid_db = finite_clamp(eq.mid_db, -12.0, 12.0, 0.0);
        eq.high_db = finite_clamp(eq.high_db, -12.0, 12.0, 0.0);
    }

    pub(crate) fn normalize_inserts(inserts: &mut [InsertFxSlot]) {
        for insert in inserts {
            insert.mix = finite_clamp(insert.mix, 0.0, 1.0, 0.5);
            insert.amount = finite_clamp(insert.amount, 0.0, 1.0, 0.35);
            Self::normalize_eq(&mut insert.eq);
            insert.time_seconds = finite_clamp(insert.time_seconds, 0.01, 1.0, 0.32);
            insert.time_steps = finite_clamp(insert.time_steps, 0.01, 256.0, 1.0);
            insert.feedback = finite_clamp(insert.feedback, 0.0, 1.0, 0.35);
            insert.rate_hz = finite_clamp(insert.rate_hz, 0.05, 8.0, 0.8);
            insert.depth = finite_clamp(insert.depth, 0.0, 1.0, 0.5);
            insert.room_size = finite_clamp(insert.room_size, 0.0, 1.0, 0.55);
            insert.drive = finite_clamp(insert.drive, 0.0, 1.0, 0.5);
            if insert.name.trim().is_empty() {
                insert.enabled = false;
            }
        }
    }

    /// True when some track is soloed.
    pub fn track_solo_active(&self) -> bool {
        self.tracks.iter().any(|track| track.solo)
    }

    /// The order tracks are processed in: children before their folder.
    ///
    /// # Returns
    ///
    /// Every track exactly once, with a folder appearing after all of its
    /// descendants so the folder chain can run on the finished submix.
    pub fn processing_order(&self) -> Vec<TrackId> {
        fn walk(
            tracks: &[Track],
            parent: Option<TrackId>,
            out: &mut Vec<TrackId>,
            guard: &mut [bool],
        ) {
            for (index, track) in tracks.iter().enumerate() {
                if track.parent != parent || guard.get(index).copied().unwrap_or(true) {
                    continue;
                }
                guard[index] = true;
                walk(tracks, Some(index), out, guard);
                out.push(index);
            }
        }
        let mut out = Vec::with_capacity(self.tracks.len());
        let mut guard = vec![false; self.tracks.len()];
        walk(&self.tracks, None, &mut out, &mut guard);
        // Tracks inside a parent cycle are unreachable from the roots.
        for index in 0..self.tracks.len() {
            if !guard[index] {
                out.push(index);
            }
        }
        out
    }

    /// The tracks the arrangement and mixer show, top to bottom, honouring
    /// collapsed folders.
    ///
    /// # Returns
    ///
    /// `(track index, depth)` pairs in display order.
    pub fn visible_tracks(&self) -> Vec<(TrackId, usize)> {
        fn walk(
            tracks: &[Track],
            parent: Option<TrackId>,
            depth: usize,
            out: &mut Vec<(TrackId, usize)>,
            guard: &mut [bool],
        ) {
            for (index, track) in tracks.iter().enumerate() {
                if track.parent != parent || guard.get(index).copied().unwrap_or(true) {
                    continue;
                }
                guard[index] = true;
                out.push((index, depth));
                if !track.collapsed {
                    walk(tracks, Some(index), depth + 1, out, guard);
                }
            }
        }
        let mut out = Vec::new();
        let mut guard = vec![false; self.tracks.len()];
        walk(&self.tracks, None, 0, &mut out, &mut guard);
        out
    }
}

/// The track a target belongs to, if it addresses one.
///
/// # Parameters
///
/// - `target`: the parameter address to inspect.
pub fn target_track(target: ParameterTarget) -> Option<TrackId> {
    match target {
        ParameterTarget::Track { track_id, .. }
        | ParameterTarget::InsertEffect { track_id, .. }
        | ParameterTarget::Clip { track_id, .. } => Some(track_id),
        _ => None,
    }
}
