//! Pointer geometry and small palette helpers.

use super::*;

pub(crate) fn rack_step_at(
    pointer_x: f32,
    grid_left: f32,
    start_step: u64,
    step_width: f32,
    visible_steps: u64,
) -> u64 {
    let last_column = visible_steps.saturating_sub(1);
    let column = ((pointer_x - grid_left) / step_width.max(1.0))
        .floor()
        .clamp(0.0, last_column as f32) as u64;
    start_step.saturating_add(column)
}

pub(crate) fn rack_step_span(start: u64, end: u64) -> (u64, u64) {
    (start.min(end), start.max(end))
}

/// True when two clips play the same material, so a drag can keep the audio
/// runtime instead of reloading it.
pub(crate) fn clips_match(left: &Clip, right: &Clip) -> bool {
    left.audio_path == right.audio_path && left.notes == right.notes
}

/// Shared editing-tool strip so the piano roll and the clip arrangement expose
/// the same tools. The arrangement omits Join, which only merges notes.
pub(crate) fn tool_palette(ui: &mut egui::Ui, tool: &mut PianoTool, include_join: bool) {
    let pen = ("P", "Pen", PianoTool::Pen);
    let select = ("S", "Select", PianoTool::Select);
    let brush = ("B", "Brush", PianoTool::Brush);
    let join = ("J", "Join", PianoTool::Join);
    let cut = ("C", "Cut", PianoTool::Cut);
    let tools: &[(&str, &str, PianoTool)] = if include_join {
        &[pen, select, brush, join, cut]
    } else {
        &[pen, select, brush, cut]
    };
    for (key, label, candidate) in tools {
        if ui
            .selectable_label(*tool == *candidate, format!("{key} · {label}"))
            .clicked()
        {
            *tool = *candidate;
        }
    }
}

/// Cursor kind painted for the active tool, shared by every editing surface.
pub(crate) fn tool_cursor_kind(tool: PianoTool, hovering_edge: bool) -> &'static str {
    match (tool, hovering_edge) {
        (_, true) => "resize",
        (PianoTool::Pen, false) => "pen",
        (PianoTool::Select, false) => "move",
        (PianoTool::Brush, false) => "brush",
        (PianoTool::Join, false) => "join",
        (PianoTool::Cut, false) => "cut",
    }
}
