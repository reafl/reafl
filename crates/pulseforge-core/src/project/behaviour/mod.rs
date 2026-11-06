//! What a [`Project`] can do: normalising, automation lookup, lane edits.
//!
//! The methods are grouped by topic so each file stays readable:
//! [`normalize`] clamps every numeric field, [`automation`] reads automation
//! and parameter values, [`lanes`] reorders and normalises channels, and
//! [`steps`] edits the step sequencer and the pattern list.

mod automation;
mod lanes;
mod normalize;
mod steps;

use super::*;

pub fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

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
