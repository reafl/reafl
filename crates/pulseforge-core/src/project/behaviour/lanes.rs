//! Lane and automation ordering, solo helpers and normalisation of inserts.

use super::*;

impl Project {
    pub fn add_lane(&mut self) {
        let old_lane_count = self.lanes.len();
        let new_len = old_lane_count + 1;
        for track in &mut self.automation_tracks {
            if let ParameterTarget::InsertEffect { channel_id, .. } = &mut track.target {
                if *channel_id == old_lane_count {
                    *channel_id = new_len;
                }
            }
        }
        self.lanes.push(ArrangementLane {
            name: format!("Track {new_len:02}"),
            ..ArrangementLane::default()
        });
        for row in &mut self.sends {
            row.insert(old_lane_count, 0.0);
        }
        self.sends.insert(old_lane_count, vec![0.0; new_len + 1]);
        self.sends[old_lane_count][new_len] = 0.25;
        self.normalize();
    }

    /// Moves track `from` to position `to`, keeping mixer sends, instrument
    /// routing and insert automation targets pointing at the same channels.
    pub fn move_lane(&mut self, from: usize, to: usize) {
        let lane_count = self.lanes.len();
        if from >= lane_count || to >= lane_count || from == to {
            return;
        }
        let master_index = lane_count;
        let lane = self.lanes.remove(from);
        self.lanes.insert(to, lane);
        // Permute the send matrix the same way, keeping the master row/column
        // last so audio routing still matches the new index order.
        if self.sends.len() == lane_count + 1
            && self.sends.iter().all(|row| row.len() == lane_count + 1)
        {
            let mut rows: Vec<Vec<f32>> = self.sends[..lane_count].to_vec();
            let row = rows.remove(from);
            rows.insert(to, row);
            let mut permuted = vec![vec![0.0; lane_count + 1]; lane_count + 1];
            for (new_row, source_row) in rows.iter().enumerate() {
                let mut values = source_row[..lane_count].to_vec();
                let value = values.remove(from);
                values.insert(to, value);
                permuted[new_row][..lane_count].copy_from_slice(&values);
                // Master column stays the last entry of each lane row.
                permuted[new_row][lane_count] = source_row[master_index];
            }
            // Master row follows the same column permutation.
            let master_row = self.sends[master_index].clone();
            let mut values = master_row[..lane_count].to_vec();
            let value = values.remove(from);
            values.insert(to, value);
            permuted[master_index][..lane_count].copy_from_slice(&values);
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
        for instrument in &mut self.instruments {
            instrument.default_mixer_lane =
                remap(instrument.default_mixer_lane.min(lane_count - 1));
        }
        for track in &mut self.automation_tracks {
            if let ParameterTarget::InsertEffect { channel_id, .. } = &mut track.target {
                *channel_id = remap((*channel_id).min(master_index));
            }
        }
        self.normalize();
    }

    /// Reorders an automation lane so grouped rows can be rearranged.
    pub fn move_automation(&mut self, from: usize, to: usize) {
        if from >= self.automation_tracks.len() || to >= self.automation_tracks.len() || from == to
        {
            return;
        }
        let track = self.automation_tracks.remove(from);
        self.automation_tracks.insert(to, track);
    }

    /// Adds an automation lane, optionally positioned immediately after another
    /// lane so a parameter can carry several adjacent clips.
    pub fn add_automation_track(&mut self, track: AutomationTrack) -> usize {
        self.automation_tracks.push(track);
        self.automation_tracks.len() - 1
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

    /// True when some instrument is soloed, so playback should mute the rest.
    pub fn instrument_solo_active(&self) -> bool {
        self.instruments.iter().any(|instrument| instrument.solo)
    }

    /// True when some track is soloed.
    pub fn lane_solo_active(&self) -> bool {
        self.lanes.iter().any(|lane| lane.solo)
    }
}
