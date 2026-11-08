//! Painting the arrangement rows: track headers, power switches and clips.

use crate::*;

impl ReaflApp {
    /// Draws every visible arrangement row and applies the context-menu and
    /// duplication requests the interactions collected.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI, used to register the per-row interactions.
    /// - `context`: the frame context, for pointer deltas and cursor icons.
    /// - `painter`: the painter of the timeline surface.
    /// - `geo`: the geometry of the current frame.
    /// - `rows`: the arrangement layout for the current project.
    pub(crate) fn paint_timeline_rows(
        &mut self,
        ui: &mut egui::Ui,
        context: &egui::Context,
        painter: &egui::Painter,
        geo: TimelineGeometry,
        rows: &[RowSpan],
    ) {
        let track_count = self.project.tracks.len();
        let mut automation_changed = false;
        let mut clip_context_action = None;
        let mut duplicate_clip_request = None;
        for span in rows.iter().skip(geo.first_lane).copied() {
            let y = geo.grid_top + span.top;
            let row_height = span.height;
            let row_kind = span.row;
            if let ArrangementRow::Automation(track_id) = row_kind {
                let row = egui::Rect::from_min_size(
                    egui::pos2(geo.rect.left(), y),
                    egui::vec2(geo.rect.width(), row_height),
                );
                automation_changed |= self.render_automation_arrangement_row(
                    ui,
                    context,
                    painter,
                    track_id,
                    row,
                    TimelineGrid {
                        timeline_left: geo.rect.left(),
                        grid_left: geo.left,
                        start_step: geo.start_step,
                        step_width: geo.step_width,
                    },
                );
                continue;
            }
            let ArrangementRow::Lane(lane_id) = row_kind else {
                unreachable!()
            };
            if lane_id >= track_count {
                continue;
            }
            let row = egui::Rect::from_min_size(
                egui::pos2(geo.rect.left(), y),
                egui::vec2(geo.rect.width(), row_height),
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
            let collapsed = self.project.tracks[lane_id].automation_collapsed;
            painter.text(
                egui::pos2(geo.rect.left() + 24.0, y + row_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &self.project.tracks[lane_id].name,
                egui::FontId::proportional(13.0),
                lane_color(lane_id),
            );
            let (lane_enabled, lane_locked, lane_solo) = {
                let lane = &self.project.tracks[lane_id];
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
                egui::pos2(geo.rect.left(), y),
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
                    egui::pos2(geo.rect.left() + 2.0, y + row_height * 0.5 - 7.0),
                    egui::vec2(14.0, 14.0),
                );
                let arrow = ui.interact(
                    arrow_rect,
                    egui::Id::new(("automation_arrow", lane_id)),
                    egui::Sense::click(),
                );
                paint_icon(
                    painter,
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
                egui::pos2(geo.left - 22.0, y + row_height * 0.5 - 8.0),
                egui::vec2(18.0, 16.0),
            );
            if button_rect.right() <= geo.left {
                let intent = power_switch_at(
                    ui,
                    button_rect,
                    egui::Id::new(("lane_power", lane_id)),
                    &PowerSwitch {
                        name: &self.project.tracks[lane_id].name,
                        enabled: lane_enabled,
                        locked: lane_locked,
                        solo: lane_solo,
                        any_solo: false,
                        color: self.ui_accent(),
                        size: egui::vec2(18.0, 16.0),
                    },
                );
                if intent.toggle {
                    self.project.tracks[lane_id].enabled = !lane_enabled;
                }
                if intent.lock {
                    self.project.tracks[lane_id].locked = !lane_locked;
                }
                if intent.solo {
                    self.project.tracks[lane_id].solo = !lane_solo;
                }
                if intent.any() {
                    self.push_history();
                    self.sync_project();
                    self.status = format!(
                        "{} {}.",
                        self.project.tracks[lane_id].name,
                        if self.project.tracks[lane_id].enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                }
            }
            painter.line_segment(
                [egui::pos2(geo.left, y), egui::pos2(geo.rect.right(), y)],
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
                egui::pos2(geo.rect.left(), y + row_height - 4.0),
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
                if let Some(track) = self.project.tracks.get_mut(lane_id) {
                    track.height = (track.height + delta).clamp(34.0, 260.0);
                }
                self.track_resize_dirty = true;
            }
            if resize_response.drag_stopped() && self.track_resize_dirty {
                self.track_resize_dirty = false;
                self.sync_project();
            }

            for (clip_index, clip) in self.project.tracks[lane_id].clips.iter().enumerate() {
                let clip_end = clip.start_step.saturating_add(clip.length_steps);
                if clip_end <= geo.start_step
                    || clip.start_step
                        >= geo
                            .start_step
                            .saturating_add(geo.bars_visible.saturating_mul(geo.steps_per_bar))
                {
                    continue;
                }
                let clip_visible_start = clip.start_step.max(geo.start_step);
                let clip_visible_end = clip_end.min(geo.view_end_step);
                let visible_left = geo.left
                    + (clip_visible_start.saturating_sub(geo.start_step) as f32) * geo.step_width;
                let visible_right = geo.left
                    + (clip_visible_end.saturating_sub(geo.start_step) as f32) * geo.step_width;
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
                if clip_response.double_clicked() && clip.is_audio() {
                    self.sample_clip_window = Some((lane_id, clip_index));
                }
                if clip_response.clicked() {
                    if context.input(|input| input.modifiers.shift) {
                        duplicate_clip_request = Some((lane_id, clip_index));
                        continue;
                    }
                    self.selected_lane = lane_id;
                    self.selection = Some(Selection::Clip {
                        track: lane_id,
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
                if clip.is_note_clip() {
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
                let color = if clip.is_audio() {
                    active_theme(context).color(active_theme(context).note)
                } else {
                    lane_color(lane_id)
                };
                // Clips in the multi-selection are drawn lifted as well, so a
                // range selected with ctrl reads as one block.
                let selected = self.is_selected(Selection::Clip {
                    track: lane_id,
                    clip: clip_index,
                });
                let lift = if selected {
                    self.clip_lift.value()
                } else {
                    0.0
                };
                let clip_rect = clip_rect.translate(egui::vec2(0.0, -lift * 2.0));
                painter.rect_filled(clip_rect, 5.0, color.gamma_multiply(0.72 + lift * 0.12));
                draw_clip_contents(
                    painter,
                    clip_rect,
                    clip,
                    clip_visible_start,
                    clip_visible_end,
                    geo.step_width,
                    geo.start_step,
                    geo.left,
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
                let name = match &clip.audio_path {
                    Some(path) => std::path::Path::new(path)
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Audio")
                        .to_owned(),
                    None => clip.name.clone(),
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
    }
}
