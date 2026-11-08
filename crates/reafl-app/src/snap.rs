//! Quantisation helpers for the editors.

use super::*;

pub(crate) fn clip_label(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("audio clip")
}

pub(crate) fn snap_label(snap_ticks: u64) -> &'static str {
    match snap_ticks {
        1 => "1/16 step",
        2 => "1/8 step",
        4 => "1/4 step",
        8 => "1/2 step",
        16 => "1 step",
        32 => "2 steps",
        64 => "4 steps",
        _ => "8 steps",
    }
}

pub(crate) fn snap_arrangement_step(step: u64, snap_ticks: u64) -> u64 {
    let snap_steps = snap_ticks.div_ceil(project::TICKS_PER_STEP).max(1);
    step.saturating_add(snap_steps / 2) / snap_steps * snap_steps
}

pub(crate) fn piano_snap_tick(tick: u64, snap_ticks: u64, alt: bool) -> u64 {
    if alt {
        tick
    } else {
        let snap_ticks = snap_ticks.max(1);
        tick.saturating_add(snap_ticks / 2) / snap_ticks * snap_ticks
    }
}

pub(crate) fn resize_note_start(
    origin_start: u64,
    origin_duration: u64,
    pointer_step: u64,
    step: u64,
) -> (u64, u64) {
    let original_end = origin_start.saturating_add(origin_duration.max(1));
    let delta = step as i128 - pointer_step as i128;
    let new_start =
        (origin_start as i128 + delta).clamp(0, original_end.saturating_sub(1) as i128) as u64;
    (new_start, original_end.saturating_sub(new_start).max(1))
}
