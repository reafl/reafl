//! Pointer and wheel interaction of the arrangement timeline.

use crate::*;

impl PulseForgeApp {
    /// Turns clicks, drags and the wheel over the timeline into selection,
    /// clip gestures, painting, slicing and panning.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI, used for the tool cursor.
    /// - `context`: the frame context, for the raw pointer state.
    /// - `painter`: the painter of the timeline surface.
    /// - `response`: the response of the timeline surface.
    /// - `geo`: the geometry of the current frame.
    /// - `rows`: the arrangement layout for the current project.
    pub(crate) fn handle_timeline_input(
        &mut self,
        ui: &mut egui::Ui,
        context: &egui::Context,
        painter: &egui::Painter,
        response: egui::Response,
        geo: TimelineGeometry,
        rows: &[RowSpan],
    ) {
        let (
            middle_down,
            primary_pressed,
            primary_down,
            primary_released,
            secondary_pressed,
            alt_snap,
        ) = context.input(|input| {
            (
                input.pointer.button_down(egui::PointerButton::Middle),
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_down(egui::PointerButton::Primary),
                input.pointer.button_released(egui::PointerButton::Primary),
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.modifiers.alt,
            )
        });
        // Right-click enters delete mode; dropping the selection first keeps the
        // following action local to what is under the pointer.
        if secondary_pressed && response.hovered() {
            self.clear_selection();
        }
        // Holding Alt disables snapping here just like in the piano roll.
        let arrangement_snap = if alt_snap {
            1
        } else {
            self.project.snap_ticks.max(1)
        };

        if middle_down && response.hovered() {
            let delta_steps =
                (-context.input(|input| input.pointer.delta().x) / geo.step_width).round() as i64;
            self.arrangement_pan_steps = self
                .arrangement_pan_steps
                .saturating_add_signed(delta_steps);
            let delta_lanes =
                (-context.input(|input| input.pointer.delta().y) / LANE_HEIGHT).round() as isize;
            self.arrangement_pan_lanes = self
                .arrangement_pan_lanes
                .saturating_add_signed(delta_lanes);
        }

        let pointer = context.input(|input| input.pointer.interact_pos());
        // Only react to raw pointer input while this surface owns the pointer.
        // Without this check, clicks on a floating window above the timeline
        // also reached the timeline underneath.
        if response.contains_pointer()
            && let Some(scroll) = context.input(|input| {
                let delta = input.raw_scroll_delta.y;
                (delta.abs() > 0.01).then_some(delta)
            })
        {
            let alt = context.input(|input| input.modifiers.alt);
            if alt {
                let steps = SNAP_STEPS;
                let current = self.project.snap_ticks;
                let index = steps
                    .iter()
                    .position(|value| *value >= current)
                    .unwrap_or(0);
                let next = if scroll > 0.0 {
                    (index + 1).min(steps.len() - 1)
                } else {
                    index.saturating_sub(1)
                };
                if steps[next] != current {
                    self.project.snap_ticks = steps[next];
                    self.sync_project();
                }
            } else if context.input(|input| input.modifiers.ctrl || input.modifiers.command) {
                // Ctrl+wheel zooms; a plain wheel scrolls the track rows, which is
                // what users expect when nothing is selected.
                self.settings.zoom =
                    (self.settings.zoom * (1.0 + scroll * 0.0015)).clamp(MIN_ZOOM, MAX_ZOOM);
            } else {
                let delta_lanes = if scroll > 0.0 { -1 } else { 1 };
                self.arrangement_pan_lanes = self
                    .arrangement_pan_lanes
                    .saturating_add_signed(delta_lanes);
            }
        }
        // `contains_pointer` also reports true for a floating window drawn on
        // top, which made clicks in those windows reach the timeline below.
        let pointer_owned = response.hovered()
            || self.arrangement_gesture.is_some()
            || self.automation_gesture.is_some()
            || self.loop_drag_start.is_some();
        let pointer_in_timeline =
            pointer.filter(|position| pointer_owned && geo.rect.contains(*position));

        if self.pattern_drag.is_some()
            && let Some(position) = pointer_in_timeline
        {
            let ghost = self.drag_ghost.value();
            if ghost > 0.01 {
                let lane = geo.first_lane + arrangement_row_at(rows, position.y - geo.grid_top);
                let step = geo.start_step.saturating_add(
                    ((position.x - geo.left) / geo.step_width).floor().max(0.0) as u64,
                );
                let ghost_rect = egui::Rect::from_min_size(
                    egui::pos2(position.x + 6.0, position.y - 14.0),
                    egui::vec2(120.0, 28.0),
                );
                painter.rect_filled(
                    ghost_rect,
                    5.0,
                    egui::Color32::from_rgba_unmultiplied(
                        112,
                        208,
                        190,
                        (110.0 * anim::smoothstep(ghost)) as u8,
                    ),
                );
                painter.text(
                    ghost_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("Lane {} ? step {}", lane + 1, step),
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_gray(20),
                );
            }
        }

        if let Some(position) = pointer_in_timeline
            && position.y < geo.grid_top
            && position.x >= geo.left
        {
            let step = snap_arrangement_step(
                geo.start_step.saturating_add(
                    ((position.x - geo.left) / geo.step_width).floor().max(0.0) as u64,
                ),
                arrangement_snap,
            );
            if primary_pressed && self.mod_ctrl {
                // Ctrl+click on the ruler resets the zoom, so the user never
                // has to hunt for a slider after a deep zoom.
                self.settings.zoom = 1.0;
                self.status = "Zoom reset to 100%.".to_owned();
            } else if primary_pressed {
                self.loop_drag_start = Some(step);
                self.loop_drag_moved = false;
            } else if primary_down
                && let Some(start) = self.loop_drag_start
                && start != step
            {
                // A loop always spans at least one step, so a zero-length
                // selection cannot be created by accident.
                self.loop_drag_moved = true;
                self.arrangement_loop = Some((
                    start.min(step),
                    start.max(step).saturating_add(1).max(start.min(step) + 1),
                ));
            }
        }
        if primary_released && self.loop_drag_start.take().is_some() && self.loop_drag_moved {
            self.loop_drag_moved = false;
            if let Some((loop_start, loop_end)) = self.arrangement_loop
                && loop_end > loop_start
            {
                self.loop_enabled = true;
                self.status = format!("Loop {loop_start} - {loop_end} steps.");
            }
        }
        if let Some(position) = pointer_in_timeline {
            let display_row = geo
                .first_lane
                .saturating_add(arrangement_row_at(rows, position.y - geo.grid_top));
            let lane = match rows.get(display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => Some(lane),
                _ => None,
            };
            let step = geo
                .start_step
                .saturating_add(((position.x - geo.left) / geo.step_width).floor().max(0.0) as u64);
            let mut on_edge = false;
            if let Some(clip) = lane
                .and_then(|lane| self.project.tracks.get(lane))
                .and_then(|lane| {
                    lane.clips.iter().find(|clip| {
                        clip.start_step <= step
                            && step < clip.start_step.saturating_add(clip.length_steps)
                    })
                })
            {
                let clip_start_x = geo.left
                    + clip.start_step.saturating_sub(geo.start_step) as f32 * geo.step_width;
                let clip_end_x = geo.left
                    + (clip.start_step.saturating_add(clip.length_steps) - geo.start_step) as f32
                        * geo.step_width;
                on_edge = (position.x - clip_end_x).abs() <= 8.0
                    || (clip.start_step >= geo.start_step
                        && (position.x - clip_start_x).abs() <= 8.0);
            }
            if position.x >= geo.left && position.y >= geo.grid_top {
                let kind = tool_cursor_kind(self.arrangement_tool, on_edge);
                let icon = match kind {
                    "resize" => egui::CursorIcon::ResizeHorizontal,
                    "cut" => egui::CursorIcon::VerticalText,
                    "brush" => egui::CursorIcon::Cell,
                    "pen" => egui::CursorIcon::Crosshair,
                    _ => egui::CursorIcon::PointingHand,
                };
                context.set_cursor_icon(icon);
                paint_svg_cursor(ui, kind, position);
                // The cut tool shows the exact split point, matching the piano
                // roll's preview.
                if self.arrangement_tool == PianoTool::Cut {
                    let cut_x =
                        geo.left + step.saturating_sub(geo.start_step) as f32 * geo.step_width;
                    if cut_x >= geo.left && cut_x <= geo.rect.right() {
                        let stroke =
                            egui::Stroke::new(1.4_f32, egui::Color32::from_rgb(232, 96, 88));
                        let mut y = geo.grid_top;
                        while y < geo.rect.bottom() {
                            painter.line_segment(
                                [
                                    egui::pos2(cut_x, y),
                                    egui::pos2(cut_x, (y + 5.0).min(geo.rect.bottom())),
                                ],
                                stroke,
                            );
                            y += 10.0;
                        }
                    }
                }
            }
        }
        if primary_pressed
            && let Some(position) = pointer_in_timeline
            && position.x >= geo.left
        {
            let display_row = geo
                .first_lane
                .saturating_add(arrangement_row_at(rows, position.y - geo.grid_top));
            let lane = match rows.get(display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => Some(lane),
                _ => None,
            };
            let step = geo
                .start_step
                .saturating_add(((position.x - geo.left) / geo.step_width).floor().max(0.0) as u64);
            let lane = lane.filter(|lane| *lane < self.project.tracks.len());
            let hit_clip = lane.and_then(|lane| {
                self.project.tracks[lane]
                    .clips
                    .iter()
                    .enumerate()
                    .find(|(_, clip)| {
                        clip.start_step <= step
                            && step < clip.start_step.saturating_add(clip.length_steps)
                    })
                    .map(|(index, clip)| (lane, index, clip.clone()))
            });
            if self.arrangement_tool == PianoTool::Select {
                // Select: click selects, click on empty space clears everything.
                match hit_clip.clone() {
                    Some((lane, clip_index, clip)) => {
                        self.selected_lane = lane;
                        self.arrangement_source = Some(clip_placement(&clip));
                        // Selecting a clip also targets its slot, so the piano
                        // roll and clip picker follow the click.
                        self.selected_clip = clip_index;
                        if self.mod_shift {
                            // Shift+click duplicates instead of extending the
                            // selection; the copy is selected so it can be dragged
                            // straight away.
                            self.duplicate_clip(lane, clip_index);
                            self.status = "Duplicated clip.".to_owned();
                        } else {
                            self.status =
                                format!("Selected clip {} on lane {}.", clip_index + 1, lane + 1);
                            let (shift, ctrl) = self.selection_modifiers();
                            self.select_item(
                                Selection::Clip {
                                    track: lane,
                                    clip: clip_index,
                                },
                                shift,
                                ctrl,
                            );
                        }
                    }
                    None => {
                        if !self.mod_shift && !self.mod_ctrl {
                            self.clear_selection();
                        }
                        self.box_select_start = pointer_in_timeline;
                    }
                }
            } else if self.arrangement_tool == PianoTool::Cut {
                if let Some(lane) = lane {
                    self.push_history();
                    if self.slice_clip_at(lane, step, arrangement_snap) {
                        self.sync_project();
                    }
                }
            } else if self.arrangement_tool == PianoTool::Brush {
                self.push_history();
                self.arrangement_painting = true;
            } else {
                self.push_history();
            }
            if let Some((lane, clip_index, clip)) = hit_clip
                .filter(|_| self.arrangement_tool != PianoTool::Cut)
                .filter(|_| !self.mod_ctrl)
            {
                // Ctrl+click only extends the selection; starting a drag here
                // would move the clip instead of adding it to the range.
                self.selected_lane = lane;
                self.arrangement_source = Some(clip_placement(&clip));
                if self.arrangement_tool != PianoTool::Select {
                    self.select_item(
                        Selection::Clip {
                            track: lane,
                            clip: clip_index,
                        },
                        false,
                        false,
                    );
                }
                let clip_end = clip.start_step.saturating_add(clip.length_steps);
                let clip_start_x = geo.left
                    + clip.start_step.saturating_sub(geo.start_step) as f32 * geo.step_width;
                let resizing_start =
                    clip.start_step >= geo.start_step && (position.x - clip_start_x).abs() <= 8.0;
                self.arrangement_gesture = Some(ArrangementGesture {
                    lane,
                    start_step: clip.start_step,
                    pointer_offset_steps: step.saturating_sub(clip.start_step),
                    source: clip_placement(&clip),
                    resizing: resizing_start || clip_end - step <= 1,
                    resizing_start,
                    resize_end_step: clip_end,
                    changed: false,
                });
            } else if self.arrangement_tool == PianoTool::Pen
                && let Some(lane) = lane
                && let Some(source) = self.arrangement_source.clone()
            {
                // Pen on empty space places the active source, matching the
                // "last clicked item" workflow of the playlist.
                let snapped = snap_arrangement_step(step, arrangement_snap);
                self.place_source_clip(lane, snapped, source);
                self.sync_project();
            }
        }

        // Right-drag deletes clips, mirroring the piano roll's delete mode. A
        // plain right-click still opens the context menu because a menu needs a
        // click, not a drag.
        if response.dragged_by(egui::PointerButton::Secondary)
            && let Some(position) = pointer_in_timeline
        {
            let display_row = geo
                .first_lane
                .saturating_add(arrangement_row_at(rows, position.y - geo.grid_top));
            let lane = match rows.get(display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => Some(lane),
                _ => None,
            };
            let step = geo
                .start_step
                .saturating_add(((position.x - geo.left) / geo.step_width).floor().max(0.0) as u64);
            if let Some(lane) = lane.filter(|lane| *lane < self.project.tracks.len())
                && let Some(index) = self.project.tracks[lane].clips.iter().position(|clip| {
                    clip.start_step <= step
                        && step < clip.start_step.saturating_add(clip.length_steps)
                })
            {
                self.push_history();
                self.project.tracks[lane].clips.remove(index);
                self.selection = None;
                self.sync_project();
                self.status = "Deleted clip (right-drag).".to_owned();
            }
        }

        if primary_down
            && self.arrangement_tool == PianoTool::Select
            && let (Some(origin), Some(position)) = (self.box_select_start, pointer_in_timeline)
        {
            let band = egui::Rect::from_two_pos(origin, position);
            if band.width() > 4.0 || band.height() > 4.0 {
                painter.rect_filled(
                    band,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(112, 208, 190, 40),
                );
                painter.rect_stroke(
                    band,
                    0.0,
                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(112, 208, 190)),
                    egui::StrokeKind::Inside,
                );
                for (lane, lane_data) in self.project.tracks.iter().enumerate() {
                    for (clip_index, clip) in lane_data.clips.iter().enumerate() {
                        let start_x = geo.left
                            + (clip.start_step as f64 - geo.start_step as f64) as f32
                                * geo.step_width;
                        let end_x = geo.left
                            + (clip.start_step.saturating_add(clip.length_steps) as f64
                                - geo.start_step as f64) as f32
                                * geo.step_width;
                        let row_index = rows.iter().position(
                            |span| matches!(span.row, ArrangementRow::Lane(id) if id == lane),
                        );
                        let Some(row_index) = row_index else {
                            continue;
                        };
                        let top = geo.grid_top + rows[row_index].top + 5.0;
                        let rect = egui::Rect::from_min_max(
                            egui::pos2(start_x, top),
                            egui::pos2(end_x, top + rows[row_index].height - 10.0),
                        );
                        if band.intersects(rect) {
                            let item = Selection::Clip {
                                track: lane,
                                clip: clip_index,
                            };
                            if !self.is_selected(item) {
                                self.extra_selection.push(item);
                            }
                        }
                    }
                }
            }
        }
        if primary_released {
            self.box_select_start = None;
        }

        if primary_down
            && self.arrangement_tool == PianoTool::Brush
            && self.arrangement_painting
            && let Some(position) = pointer_in_timeline
            && position.x >= geo.left
            && position.y >= geo.grid_top
        {
            let display_row = geo
                .first_lane
                .saturating_add(arrangement_row_at(rows, position.y - geo.grid_top));
            let lane = match rows.get(display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => lane,
                _ => usize::MAX,
            };
            let step = snap_arrangement_step(
                geo.start_step.saturating_add(
                    ((position.x - geo.left) / geo.step_width).floor().max(0.0) as u64,
                ),
                arrangement_snap,
            );
            if lane < self.project.tracks.len()
                && let Some(source) = self.arrangement_source.clone()
            {
                let occupied = self.project.tracks[lane].clips.iter().any(|clip| {
                    clip.start_step <= step
                        && step < clip.start_step.saturating_add(clip.length_steps)
                });
                if !occupied {
                    let painted = self.place_source_clip(lane, step, source);
                    if painted.is_some() {
                        self.sync_project();
                    }
                }
            }
        }

        if primary_down
            && let (Some(mut gesture), Some(position)) =
                (self.arrangement_gesture.clone(), pointer_in_timeline)
            && position.x >= geo.left
        {
            let target_display_row = geo
                .first_lane
                .saturating_add(arrangement_row_at(rows, position.y - geo.grid_top));
            let target_lane = match rows.get(target_display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => lane,
                _ => usize::MAX,
            };
            let pointer_step = snap_arrangement_step(
                geo.start_step.saturating_add(
                    ((position.x - geo.left) / geo.step_width).floor().max(0.0) as u64,
                ),
                arrangement_snap,
            );
            if target_lane == gesture.lane && gesture.resizing {
                if let Some(clip) =
                    self.project.tracks[gesture.lane]
                        .clips
                        .iter_mut()
                        .find(|clip| {
                            clip.start_step == gesture.start_step
                                && clip_placement(clip) == gesture.source
                        })
                {
                    let (new_start, new_length) = if gesture.resizing_start {
                        let new_start = pointer_step.min(gesture.resize_end_step.saturating_sub(1));
                        (
                            new_start,
                            gesture.resize_end_step.saturating_sub(new_start).max(1),
                        )
                    } else {
                        (
                            gesture.start_step,
                            pointer_step
                                .saturating_sub(gesture.start_step)
                                .saturating_add(1)
                                .max(1),
                        )
                    };
                    if new_start != clip.start_step || new_length != clip.length_steps {
                        if new_start > clip.start_step {
                            clip.offset_steps = clip
                                .offset_steps
                                .saturating_add(new_start - clip.start_step);
                        } else {
                            clip.offset_steps = clip
                                .offset_steps
                                .saturating_sub(clip.start_step - new_start);
                        }
                        clip.start_step = new_start;
                        clip.length_steps = new_length;
                        gesture.start_step = new_start;
                        gesture.changed = true;
                        self.arrangement_gesture = Some(gesture);
                    }
                }
            } else if target_lane < self.project.tracks.len() {
                let target_start = snap_arrangement_step(
                    pointer_step.saturating_sub(gesture.pointer_offset_steps),
                    arrangement_snap,
                );
                if (target_lane != gesture.lane || target_start != gesture.start_step)
                    && let Some(old_lane) = self.project.tracks.get_mut(gesture.lane)
                    && let Some(index) = old_lane.clips.iter().position(|clip| {
                        clip.start_step == gesture.start_step
                            && clip_placement(clip) == gesture.source
                    })
                {
                    let mut clip = old_lane.clips.remove(index);
                    clip.start_step = target_start;
                    self.project.tracks[target_lane].clips.push(clip);
                    self.project.tracks[target_lane]
                        .clips
                        .sort_by_key(|clip| clip.start_step);
                    gesture.lane = target_lane;
                    gesture.start_step = target_start;
                    gesture.changed = true;
                    self.arrangement_gesture = Some(gesture);
                }
            }
        }

        if primary_released {
            self.arrangement_painting = false;
        }

        if primary_released
            && let Some(gesture) = self.arrangement_gesture.take()
            && gesture.changed
        {
            self.selected_lane = gesture.lane;
            let clip_index = self.project.tracks[gesture.lane]
                .clips
                .iter()
                .position(|clip| {
                    clip.start_step == gesture.start_step && clip_placement(clip) == gesture.source
                });
            if let Some(clip) = clip_index {
                self.selection = Some(Selection::Clip {
                    track: gesture.lane,
                    clip,
                });
            }
            self.sync_project();
        }

        if primary_released
            && let (Some(source), Some(position)) = (self.pattern_drag.take(), pointer_in_timeline)
            && position.x >= geo.left
        {
            let lane = geo
                .first_lane
                .saturating_add(arrangement_row_at(rows, position.y - geo.grid_top));
            let visible_step = ((position.x - geo.left) / geo.step_width).floor().max(0.0) as u64;
            let start_step = snap_arrangement_step(
                geo.start_step.saturating_add(visible_step),
                arrangement_snap,
            );
            if lane < self.project.tracks.len() {
                self.push_history();
                self.arrangement_source = Some(source.clone());
                self.place_source_clip(lane, start_step, source);
            }
        } else if primary_released && self.pattern_drag.take().is_some() {
            self.status = "Drop the clip over an arrangement lane.".to_owned();
        }
    }
}
