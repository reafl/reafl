//! What a [`Project`] can do: normalising, automation lookup, track edits.
//!
//! The methods are grouped by topic so each file stays readable:
//! [`normalize`] clamps every numeric field and keeps the track tree valid,
//! [`automation`] reads automation and parameter values, and [`tracks`]
//! reorders, groups and removes tracks and edits their clips.

mod automation;
mod normalize;
mod tracks;

pub use tracks::target_track;

use super::*;

/// Clamps `value` into `min..=max`, falling back to `fallback` when it is not
/// a finite number.
pub fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

/// Makes the send matrix square and keeps every destination's absolute gain sum
/// below [`MAX_SEND_GAIN_SUM`].
pub fn normalize_sends(sends: &mut [Vec<f32>]) {
    let lane_count = sends.len();
    for row in sends.iter_mut() {
        row.resize(lane_count, 0.0);
        for gain in row.iter_mut() {
            *gain = finite_clamp(*gain, 0.0, MAX_SEND_GAIN_SUM, 0.0);
        }
    }
    let totals: Vec<f32> = (0..lane_count)
        .map(|destination| sends.iter().map(|row| row[destination].abs()).sum())
        .collect();
    for row in sends {
        for (destination, gain) in row.iter_mut().enumerate() {
            if totals[destination] > MAX_SEND_GAIN_SUM {
                *gain *= MAX_SEND_GAIN_SUM / totals[destination];
            }
        }
    }
}

/// Removes every send into and out of `channel`.
pub fn disconnect_channel(sends: &mut [Vec<f32>], channel: usize) {
    for row in sends.iter_mut() {
        if let Some(gain) = row.get_mut(channel) {
            *gain = 0.0;
        }
    }
    if let Some(row) = sends.get_mut(channel) {
        row.fill(0.0);
    }
}
