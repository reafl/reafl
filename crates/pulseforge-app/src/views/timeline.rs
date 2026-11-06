//! The timeline ruler above the arrangement lanes.

use crate::*;

impl PulseForgeApp {
    pub(crate) fn render_timeline(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let (timeline_rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter_at(timeline_rect);
        painter.rect_filled(timeline_rect, 8.0, egui::Color32::from_gray(24));

        let steps_per_bar = self.project.time_signature.steps_per_bar();
        let step_width = BAR_WIDTH * self.settings.zoom / steps_per_bar as f32;
        let left = timeline_rect.left() + LANE_LABEL_WIDTH;
        let bar_width = BAR_WIDTH * self.settings.zoom;
        let bars_visible =
            ((timeline_rect.width() - LANE_LABEL_WIDTH) / bar_width).ceil() as u64 + 1;
        let first_bar = self.arrangement_pan_steps / steps_per_bar;
        let start_step = first_bar.saturating_mul(steps_per_bar);
        let header_height = 30.0;
        let grid_top = timeline_rect.top() + header_height;
        let first_lane = self.arrangement_pan_lanes;
        // Rows have independent heights, so the visible count is discovered
        // after the layout is built instead of being divided by a constant.
        let view_end_step = start_step.saturating_add(bars_visible.saturating_mul(steps_per_bar));

        painter.text(
            egui::pos2(timeline_rect.left() + 16.0, timeline_rect.top() + 15.0),
            egui::Align2::LEFT_CENTER,
            "TRACK",
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(145),
        );
        for visible_bar in 0..bars_visible {
            let bar = first_bar.saturating_add(visible_bar);
            let x = left + visible_bar as f32 * bar_width;
            let rect = egui::Rect::from_min_size(
                egui::pos2(x, timeline_rect.top()),
                egui::vec2(bar_width, header_height),
            );
            painter.rect_filled(
                rect,
                0.0,
                if bar % 2 == 0 {
                    egui::Color32::from_gray(35)
                } else {
                    egui::Color32::from_gray(29)
                },
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("BAR {}", bar + 1),
                egui::FontId::proportional(11.0),
                egui::Color32::LIGHT_GRAY,
            );
        }
        if let Some((loop_start, loop_end)) = self.arrangement_loop {
            let x_start = left + (loop_start as f64 - start_step as f64) as f32 * step_width;
            let x_end = left + (loop_end as f64 - start_step as f64) as f32 * step_width;
            let loop_rect = egui::Rect::from_min_max(
                egui::pos2(x_start, timeline_rect.top() + 2.0),
                egui::pos2(x_end.max(x_start + 3.0), timeline_rect.top() + 7.0),
            );
            painter.rect_filled(loop_rect, 2.0, egui::Color32::from_rgb(112, 208, 190));
        }

        let lane_count = self.project.lanes.len();
        let rows = arrangement_layout(&self.project);
        let row_index_at = |offset: f32| arrangement_row_at(&rows, offset);
        let mut automation_changed = false;
        let mut clip_context_action = None;
        let mut duplicate_clip_request = None;
        for span in rows.iter().skip(first_lane).copied() {
            let y = grid_top + span.top;
            let row_height = span.height;
            let row_kind = span.row;
            if let ArrangementRow::Automation(track_id) = row_kind {
                let row = egui::Rect::from_min_size(
                    egui::pos2(timeline_rect.left(), y),
                    egui::vec2(timeline_rect.width(), row_height),
                );
                automation_changed |= self.render_automation_arrangement_row(
                    ui,
                    context,
                    &painter,
                    track_id,
                    row,
                    TimelineGrid {
                        timeline_left: timeline_rect.left(),
                        grid_left: left,
                        start_step,
                        step_width,
                    },
                );
                continue;
            }
            let ArrangementRow::Lane(lane_id) = row_kind else {
                unreachable!()
            };
            if lane_id >= lane_count {
                continue;
            }
            let row = egui::Rect::from_min_size(
                egui::pos2(timeline_rect.left(), y),
                egui::vec2(timeline_rect.width(), row_height),
            );
            painter.rect_filled(
                row,
                0.0,
                if lane_id % 2 == 0 {
                    egui::Color32::from_gray(30)
                } else {
                    egui::Color32::from_gray(27)
                },
            );
            let collapsed = self.project.lanes[lane_id].automation_collapsed;
            painter.text(
                egui::pos2(timeline_rect.left() + 24.0, y + row_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &self.project.lanes[lane_id].name,
                egui::FontId::proportional(13.0),
                lane_color(lane_id),
            );
            let (lane_enabled, lane_locked, lane_solo) = {
                let lane = &self.project.lanes[lane_id];
                (lane.enabled, lane.locked, lane.solo)
            };
            let has_automation = self
                .project
                .automation_tracks
                .iter()
                .any(|track| automation_owner_lane(&self.project, track) == lane_id);
            // Track header: rename/reorder menu and a selection target. It is
            // registered before the collapse arrow and power switch so those
            // small controls stay on top of it.
            let header_rect = egui::Rect::from_min_size(
                egui::pos2(timeline_rect.left(), y),
                egui::vec2(LANE_LABEL_WIDTH, row_height),
            );
            let header_response = ui.interact(
                header_rect,
                egui::Id::new(("track_header", lane_id)),
                egui::Sense::click(),
            );
            if header_response.clicked() {
                self.select_lane(lane_id);
            }
            if header_response.double_clicked() {
                self.selected_lane = lane_id;
                self.begin_rename_track();
            }
            // Collapse arrow only appears when automation rows exist.
            if has_automation {
                let arrow_rect = egui::Rect::from_min_size(
                    egui::pos2(timeline_rect.left() + 2.0, y + row_height * 0.5 - 7.0),
                    egui::vec2(14.0, 14.0),
                );
                let arrow = ui.interact(
                    arrow_rect,
                    egui::Id::new(("automation_arrow", lane_id)),
                    egui::Sense::click(),
                );
                paint_icon(
                    &painter,
                    arrow_rect.shrink(3.0),
                    if collapsed {
                        Icon::ArrowRight
                    } else {
                        Icon::ArrowDown
                    },
                    if arrow.hovered() {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::from_gray(160)
                    },
                );
                if arrow.clicked() {
                    self.apply_context_action(ContextAction::ToggleTrackAutomation(lane_id));
                }
            }
            // One power switch per track, drawn by the shared widget so it matches
            // the mixer and the channel rack exactly.
            let button_rect = egui::Rect::from_min_size(
                egui::pos2(left - 22.0, y + row_height * 0.5 - 8.0),
                egui::vec2(18.0, 16.0),
            );
            if button_rect.right() <= left {
                let intent = power_switch_at(
                    ui,
                    button_rect,
                    egui::Id::new(("lane_power", lane_id)),
                    &PowerSwitch {
                        name: &self.project.lanes[lane_id].name,
                        enabled: lane_enabled,
                        locked: lane_locked,
                        solo: lane_solo,
                        any_solo: false,
                        color: self.ui_accent(),
                        size: egui::vec2(18.0, 16.0),
                    },
                );
                if intent.toggle {
                    self.project.lanes[lane_id].enabled = !lane_enabled;
                }
                if intent.lock {
                    self.project.lanes[lane_id].locked = !lane_locked;
                }
                if intent.solo {
                    self.project.lanes[lane_id].solo = !lane_solo;
                }
                if intent.any() {
                    self.push_history();
                    self.sync_project();
                    self.status = format!(
                        "{} {}.",
                        self.project.lanes[lane_id].name,
                        if self.project.lanes[lane_id].enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                }
            }
            painter.line_segment(
                [egui::pos2(left, y), egui::pos2(timeline_rect.right(), y)],
                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(47)),
            );
            // Drag handle along the bottom edge for vertical resizing.
            let track_menu = ContextMenu {
                items: vec![
                    MenuItem::Action {
                        label: "Rename track (F2)".to_owned(),
                        action: ContextAction::RenameTrack(lane_id),
                    },
                    MenuItem::Separator,
                    MenuItem::Folder {
                        label: "Reorder".to_owned(),
                        items: vec![
                            MenuItem::Action {
                                label: "Move up".to_owned(),
                                action: ContextAction::MoveTrack {
                                    lane: lane_id,
                                    up: true,
                                },
                            },
                            MenuItem::Action {
                                label: "Move down".to_owned(),
                                action: ContextAction::MoveTrack {
                                    lane: lane_id,
                                    up: false,
                                },
                            },
                        ],
                    },
                    MenuItem::Action {
                        label: if collapsed {
                            "Show automation".to_owned()
                        } else {
                            "Hide automation".to_owned()
                        },
                        action: ContextAction::ToggleTrackAutomation(lane_id),
                    },
                    MenuItem::Action {
                        label: "New automation clip".to_owned(),
                        action: ContextAction::NewAutomationForTrack(lane_id),
                    },
                    MenuItem::Separator,
                    MenuItem::Action {
                        label: "Pin track".to_owned(),
                        action: ContextAction::ToggleLanePin(lane_id),
                    },
                    MenuItem::Action {
                        label: "Disconnect routing".to_owned(),
                        action: ContextAction::DisconnectChannel(lane_id),
                    },
                ],
            };
            let mut track_action = None;
            header_response.context_menu(|ui| track_action = track_menu.show(ui));
            if let Some(action) = track_action {
                clip_context_action = Some(action);
            }
            let resize_rect = egui::Rect::from_min_size(
                egui::pos2(timeline_rect.left(), y + row_height - 4.0),
                egui::vec2(LANE_LABEL_WIDTH, 5.0),
            );
            let resize_response = ui.interact(
                resize_rect,
                egui::Id::new(("track_resize", lane_id)),
                egui::Sense::drag(),
            );
            if resize_response.hovered() || resize_response.dragged() {
                context.set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if resize_response.dragged() {
                let delta = context.input(|input| input.pointer.delta().y);
                if let Some(track) = self.project.lanes.get_mut(lane_id) {
                    track.height = (track.height + delta).clamp(34.0, 260.0);
                }
                self.track_resize_dirty = true;
            }
            if resize_response.drag_stopped() && self.track_resize_dirty {
                self.track_resize_dirty = false;
                self.sync_project();
            }

            for (clip_index, clip) in self.project.lanes[lane_id].clips.iter().enumerate() {
                let clip_end = clip.start_step.saturating_add(clip.length_steps);
                if clip_end <= start_step
                    || clip.start_step
                        >= start_step.saturating_add(bars_visible.saturating_mul(steps_per_bar))
                {
                    continue;
                }
                let clip_visible_start = clip.start_step.max(start_step);
                let clip_visible_end = clip_end.min(view_end_step);
                let visible_left =
                    left + (clip_visible_start.saturating_sub(start_step) as f32) * step_width;
                let visible_right =
                    left + (clip_visible_end.saturating_sub(start_step) as f32) * step_width;
                if visible_right <= visible_left {
                    continue;
                }
                let clip_rect = egui::Rect::from_min_max(
                    egui::pos2(visible_left, y + 5.0),
                    egui::pos2(visible_right, y + row_height - 5.0),
                );
                let clip_response = ui.interact(
                    clip_rect,
                    egui::Id::new(("arrangement_clip", lane_id, clip_index)),
                    egui::Sense::click(),
                );
                if clip_response.double_clicked()
                    && matches!(&clip.source, ClipSource::Audio { .. })
                {
                    self.sample_clip_window = Some((lane_id, clip_index));
                }
                if clip_response.clicked() {
                    if context.input(|input| input.modifiers.shift) {
                        duplicate_clip_request = Some((lane_id, clip_index));
                        continue;
                    }
                    self.selected_lane = lane_id;
                    self.selection = Some(Selection::Clip {
                        lane: lane_id,
                        clip: clip_index,
                    });
                }
                let mut clip_menu_items = vec![
                    MenuItem::Action {
                        label: "Duplicate after".to_owned(),
                        action: ContextAction::DuplicateClip {
                            lane: lane_id,
                            clip: clip_index,
                        },
                    },
                    MenuItem::Action {
                        label: "Delete clip".to_owned(),
                        action: ContextAction::DeleteClip {
                            lane: lane_id,
                            clip: clip_index,
                        },
                    },
                ];
                if matches!(&clip.source, ClipSource::Pattern { .. }) {
                    clip_menu_items.push(MenuItem::Folder {
                        label: "Pattern".to_owned(),
                        items: vec![MenuItem::Action {
                            label: "Make unique".to_owned(),
                            action: ContextAction::MakeClipPatternUnique {
                                lane: lane_id,
                                clip: clip_index,
                            },
                        }],
                    });
                }
                clip_menu_items.extend([
                    MenuItem::Separator,
                    MenuItem::Folder {
                        label: "Track".to_owned(),
                        items: vec![MenuItem::Action {
                            label: "Select mixer channel".to_owned(),
                            action: ContextAction::SelectChannel(lane_id),
                        }],
                    },
                ]);
                let clip_menu = ContextMenu {
                    items: clip_menu_items,
                };
                clip_response.context_menu(|ui| clip_context_action = clip_menu.show(ui));
                let color = match &clip.source {
                    ClipSource::Pattern { pattern_id } => lane_color(*pattern_id),
                    ClipSource::Audio { .. } => {
                        active_theme(context).color(active_theme(context).note)
                    }
                };
                // Clips in the multi-selection are drawn lifted as well, so a
                // range selected with ctrl reads as one block.
                let selected = self.is_selected(Selection::Clip {
                    lane: lane_id,
                    clip: clip_index,
                });
                let lift = if selected { self.clip_lift.value() } else { 0.0 };
                let clip_rect = clip_rect.translate(egui::vec2(0.0, -lift * 2.0));
                painter.rect_filled(clip_rect, 5.0, color.gamma_multiply(0.72 + lift * 0.12));
                draw_clip_contents(
                    &painter,
                    clip_rect,
                    clip,
                    &self.project.patterns,
                    clip_visible_start,
                    clip_visible_end,
                    step_width,
                    start_step,
                    left,
                );
                painter.rect_stroke(
                    clip_rect,
                    5.0,
                    egui::Stroke::new(
                        if selected { 2.4 } else { 1.0_f32 },
                        if selected {
                            egui::Color32::WHITE
                        } else {
                            color
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                let name = match &clip.source {
                    ClipSource::Pattern { pattern_id } => self
                        .project
                        .patterns
                        .get(*pattern_id)
                        .map_or_else(|| "Pattern".to_owned(), |pattern| pattern.name.clone()),
                    ClipSource::Audio { path } => std::path::Path::new(path)
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Audio")
                        .to_owned(),
                };
                painter.text(
                    clip_rect.left_center() + egui::vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(12.0),
                    egui::Color32::WHITE,
                );
            }
        }
        if let Some(action) = clip_context_action {
            self.apply_context_action(action);
        }
        if let Some((lane, clip)) = duplicate_clip_request {
            self.duplicate_clip(lane, clip);
        }
        if automation_changed {
            self.sync_project();
        }

        for bar in 0..=bars_visible {
            let x = left + bar as f32 * bar_width;
            painter.line_segment(
                [
                    egui::pos2(x, grid_top),
                    egui::pos2(x, timeline_rect.bottom()),
                ],
                egui::Stroke::new(
                    1.0_f32,
                    if bar % 4 == 0 {
                        egui::Color32::from_gray(82)
                    } else {
                        egui::Color32::from_gray(47)
                    },
                ),
            );
        }
        // Beat lines follow the same `step_width` as the clips, so the grid
        // scales with the zoom level instead of drifting away from the content.
        if step_width * steps_per_bar as f32 > 12.0 {
            let beats_visible = bars_visible.saturating_mul(steps_per_bar);
            let beat_width = step_width * 4.0;
            for beat in 0..=beats_visible {
                if beat % 4 == 0 {
                    continue;
                }
                let x = left + beat as f32 * beat_width;
                painter.line_segment(
                    [
                        egui::pos2(x, grid_top),
                        egui::pos2(x, timeline_rect.bottom()),
                    ],
                    egui::Stroke::new(1.0_f32, egui::Color32::from_gray(38)),
                );
            }
        }
        let total_height = grid_top + rows.last().map_or(0.0, |span| span.top + span.height);
        for span in &rows {
            let y = grid_top + span.top + span.height;
            painter.line_segment(
                [
                    egui::pos2(timeline_rect.left(), y),
                    egui::pos2(timeline_rect.right(), y),
                ],
                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(48)),
            );
        }
        if self.playing {
            let playhead_step = self.playhead_step();
            let x = left + (playhead_step - start_step as f64) as f32 * step_width;
            if x >= left && x <= timeline_rect.right() {
                painter.line_segment(
                    [
                        egui::pos2(x, timeline_rect.top()),
                        egui::pos2(x, timeline_rect.bottom()),
                    ],
                    egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(255, 104, 79)),
                );
                painter.circle_filled(
                    egui::pos2(x, timeline_rect.top() + 4.0),
                    4.0,
                    egui::Color32::from_rgb(255, 104, 79),
                );
            }
        }
        if total_height < timeline_rect.bottom() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(timeline_rect.left(), total_height),
                    timeline_rect.right_bottom(),
                ),
                0.0,
                egui::Color32::from_gray(22),
            );
        }

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
                (-context.input(|input| input.pointer.delta().x) / step_width).round() as i64;
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
            pointer.filter(|position| pointer_owned && timeline_rect.contains(*position));

        if self.pattern_drag.is_some()
            && let Some(position) = pointer_in_timeline
        {
            let ghost = self.drag_ghost.value();
            if ghost > 0.01 {
                let lane = first_lane + row_index_at(position.y - grid_top);
                let step = start_step
                    .saturating_add(((position.x - left) / step_width).floor().max(0.0) as u64);
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
            && position.y < grid_top
            && position.x >= left
        {
            let step = snap_arrangement_step(
                start_step
                    .saturating_add(((position.x - left) / step_width).floor().max(0.0) as u64),
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
            let display_row = first_lane.saturating_add(row_index_at(position.y - grid_top));
            let lane = match rows.get(display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => Some(lane),
                _ => None,
            };
            let step = start_step
                .saturating_add(((position.x - left) / step_width).floor().max(0.0) as u64);
            let mut on_edge = false;
            if let Some(clip) =
                lane.and_then(|lane| self.project.lanes.get(lane))
                    .and_then(|lane| {
                        lane.clips.iter().find(|clip| {
                            clip.start_step <= step
                                && step < clip.start_step.saturating_add(clip.length_steps)
                        })
                    })
            {
                let clip_start_x =
                    left + clip.start_step.saturating_sub(start_step) as f32 * step_width;
                let clip_end_x = left
                    + (clip.start_step.saturating_add(clip.length_steps) - start_step) as f32
                        * step_width;
                on_edge = (position.x - clip_end_x).abs() <= 8.0
                    || (clip.start_step >= start_step && (position.x - clip_start_x).abs() <= 8.0);
            }
            if position.x >= left && position.y >= grid_top {
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
                    let cut_x = left + step.saturating_sub(start_step) as f32 * step_width;
                    if cut_x >= left && cut_x <= timeline_rect.right() {
                        let stroke = egui::Stroke::new(1.4_f32, egui::Color32::from_rgb(232, 96, 88));
                        let mut y = grid_top;
                        while y < timeline_rect.bottom() {
                            painter.line_segment(
                                [egui::pos2(cut_x, y), egui::pos2(cut_x, (y + 5.0).min(timeline_rect.bottom()))],
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
            && position.x >= left
        {
            let display_row = first_lane.saturating_add(row_index_at(position.y - grid_top));
            let lane = match rows.get(display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => Some(lane),
                _ => None,
            };
            let step = start_step
                .saturating_add(((position.x - left) / step_width).floor().max(0.0) as u64);
            let lane = lane.filter(|lane| *lane < self.project.lanes.len());
            let hit_clip = lane.and_then(|lane| {
                self.project.lanes[lane]
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
                        self.arrangement_source = Some(clip.source.clone());
                        // Selecting a pattern clip also selects its pattern, so
                        // the piano roll and clip picker follow the click.
                        if let ClipSource::Pattern { pattern_id } = &clip.source {
                            self.selected_pattern = *pattern_id;
                        }
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
                                    lane,
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
                self.arrangement_source = Some(clip.source.clone());
                if self.arrangement_tool != PianoTool::Select {
                    self.select_item(
                        Selection::Clip {
                            lane,
                            clip: clip_index,
                        },
                        false,
                        false,
                    );
                }
                let clip_end = clip.start_step.saturating_add(clip.length_steps);
                let clip_start_x =
                    left + clip.start_step.saturating_sub(start_step) as f32 * step_width;
                let resizing_start =
                    clip.start_step >= start_step && (position.x - clip_start_x).abs() <= 8.0;
                self.arrangement_gesture = Some(ArrangementGesture {
                    lane,
                    start_step: clip.start_step,
                    pointer_offset_steps: step.saturating_sub(clip.start_step),
                    source: clip.source.clone(),
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
            let display_row = first_lane.saturating_add(row_index_at(position.y - grid_top));
            let lane = match rows.get(display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => Some(lane),
                _ => None,
            };
            let step = start_step
                .saturating_add(((position.x - left) / step_width).floor().max(0.0) as u64);
            if let Some(lane) = lane.filter(|lane| *lane < self.project.lanes.len())
                && let Some(index) = self.project.lanes[lane].clips.iter().position(|clip| {
                    clip.start_step <= step
                        && step < clip.start_step.saturating_add(clip.length_steps)
                })
            {
                self.push_history();
                self.project.lanes[lane].clips.remove(index);
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
                for (lane, lane_data) in self.project.lanes.iter().enumerate() {
                    for (clip_index, clip) in lane_data.clips.iter().enumerate() {
                        let start_x =
                            left + (clip.start_step as f64 - start_step as f64) as f32 * step_width;
                        let end_x = left
                            + (clip.start_step.saturating_add(clip.length_steps) as f64
                                - start_step as f64) as f32
                                * step_width;
                        let row_index = rows.iter().position(
                            |span| matches!(span.row, ArrangementRow::Lane(id) if id == lane),
                        );
                        let Some(row_index) = row_index else {
                            continue;
                        };
                        let top = grid_top + rows[row_index].top + 5.0;
                        let rect = egui::Rect::from_min_max(
                            egui::pos2(start_x, top),
                            egui::pos2(end_x, top + rows[row_index].height - 10.0),
                        );
                        if band.intersects(rect) {
                            let item = Selection::Clip {
                                lane,
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
            && position.x >= left
            && position.y >= grid_top
        {
            let display_row = first_lane.saturating_add(row_index_at(position.y - grid_top));
            let lane = match rows.get(display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => lane,
                _ => usize::MAX,
            };
            let step = snap_arrangement_step(
                start_step
                    .saturating_add(((position.x - left) / step_width).floor().max(0.0) as u64),
                arrangement_snap,
            );
            if lane < self.project.lanes.len()
                && let Some(source) = self.arrangement_source.clone()
            {
                let occupied = self.project.lanes[lane].clips.iter().any(|clip| {
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
            && position.x >= left
        {
            let target_display_row = first_lane.saturating_add(row_index_at(position.y - grid_top));
            let target_lane = match rows.get(target_display_row).map(|span| span.row) {
                Some(ArrangementRow::Lane(lane)) => lane,
                _ => usize::MAX,
            };
            let pointer_step = snap_arrangement_step(
                start_step
                    .saturating_add(((position.x - left) / step_width).floor().max(0.0) as u64),
                arrangement_snap,
            );
            if target_lane == gesture.lane && gesture.resizing {
                if let Some(clip) = self.project.lanes[gesture.lane]
                    .clips
                    .iter_mut()
                    .find(|clip| {
                        clip.start_step == gesture.start_step
                            && clip_sources_match(&clip.source, &gesture.source)
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
            } else if target_lane < self.project.lanes.len() {
                let target_start = snap_arrangement_step(
                    pointer_step.saturating_sub(gesture.pointer_offset_steps),
                    arrangement_snap,
                );
                if (target_lane != gesture.lane || target_start != gesture.start_step)
                    && let Some(old_lane) = self.project.lanes.get_mut(gesture.lane)
                    && let Some(index) = old_lane.clips.iter().position(|clip| {
                        clip.start_step == gesture.start_step
                            && clip_sources_match(&clip.source, &gesture.source)
                    })
                {
                    let mut clip = old_lane.clips.remove(index);
                    clip.start_step = target_start;
                    self.project.lanes[target_lane].clips.push(clip);
                    self.project.lanes[target_lane]
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
            let clip_index = self.project.lanes[gesture.lane]
                .clips
                .iter()
                .position(|clip| {
                    clip.start_step == gesture.start_step
                        && clip_sources_match(&clip.source, &gesture.source)
                });
            if let Some(clip) = clip_index {
                self.selection = Some(Selection::Clip {
                    lane: gesture.lane,
                    clip,
                });
            }
            self.sync_project();
        }

        if primary_released
            && let (Some(source), Some(position)) = (self.pattern_drag.take(), pointer_in_timeline)
            && position.x >= left
        {
            let lane = first_lane.saturating_add(row_index_at(position.y - grid_top));
            let visible_step = ((position.x - left) / step_width).floor().max(0.0) as u64;
            let start_step =
                snap_arrangement_step(start_step.saturating_add(visible_step), arrangement_snap);
            if lane < self.project.lanes.len() {
                self.push_history();
                self.arrangement_source = Some(source.clone());
                self.place_source_clip(lane, start_step, source);
            }
        } else if primary_released && self.pattern_drag.take().is_some() {
            self.status = "Drop the clip over an arrangement lane.".to_owned();
        }
    }
}

