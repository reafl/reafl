//! One automation lane in the arrangement: curve, points and clip menu.

use crate::*;

impl ReaflApp {
    pub(crate) fn render_automation_arrangement_row(
        &mut self,
        ui: &mut egui::Ui,
        context: &egui::Context,
        painter: &egui::Painter,
        track_id: usize,
        row: egui::Rect,
        grid: TimelineGrid,
    ) -> bool {
        // Automation clips are dragged like pattern clips. Handling the drag
        // before borrowing the project keeps the gesture code readable.
        let (primary_pressed, primary_down, primary_released) = context.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_down(egui::PointerButton::Primary),
                input.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        let pointer = context.input(|input| input.pointer.interact_pos());
        let step_at = |x: f32| -> u64 {
            grid.start_step
                .saturating_add(((x - grid.grid_left) / grid.step_width).floor().max(0.0) as u64)
        };
        let Some((track_start, track_length)) = self
            .project
            .automation_tracks
            .get(track_id)
            .map(|track| (track.start_step, track.length_steps))
        else {
            return false;
        };
        let start_x =
            grid.grid_left + (track_start as f64 - grid.start_step as f64) as f32 * grid.step_width;
        let end_x = grid.grid_left
            + (track_start.saturating_add(track_length) as f64 - grid.start_step as f64) as f32
                * grid.step_width;
        // Points sit on the same pixels as the clip body, so a press that lands
        // on one must not also drag the clip.
        let point_centres: Vec<egui::Pos2> = self
            .project
            .automation_tracks
            .get(track_id)
            .map(|track| {
                let points = if track.source.edits_points() {
                    &track.points
                } else {
                    &track.modulation_points
                };
                points
                    .iter()
                    .map(|point| {
                        // Relative lanes are drawn around zero, so their points
                        // can sit below the centre line.
                        let floor = if track.source.is_relative() {
                            -1.0
                        } else {
                            0.0
                        };
                        egui::pos2(
                            grid.grid_left
                                + (point.step as f64 - grid.start_step as f64) as f32
                                    * grid.step_width,
                            row.bottom()
                                - 6.0
                                - ((point.value.clamp(floor, 1.0) - floor) / (1.0 - floor))
                                    * (row.height() - 12.0),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let on_point = |position: egui::Pos2| {
            point_centres.iter().any(|centre| {
                (centre.x - position.x).abs() <= 7.0 && (centre.y - position.y).abs() <= 7.0
            })
        };
        if primary_pressed
            && let Some(position) = pointer.filter(|position| row.contains(*position))
            && position.y <= row.top() + AUTOMATION_CLIP_GRAB_HEIGHT
            && !on_point(position)
            && position.x >= start_x - 4.0
            && position.x <= end_x + 4.0
            && position.x >= grid.grid_left
        {
            self.push_history();
            self.selection = Some(Selection::Automation(track_id));
            self.automation_gesture = Some(AutomationGesture {
                track_id,
                grab_step: step_at(position.x),
                changed: false,
            });
        }
        if primary_down
            && let Some(gesture) = self.automation_gesture
            && gesture.track_id == track_id
            && let Some(position) = pointer
        {
            let snapped = snap_arrangement_step(
                step_at(position.x).saturating_sub(gesture.grab_step),
                self.project.snap_ticks,
            );
            if let Some(track) = self.project.automation_tracks.get_mut(track_id) {
                track.start_step = snapped;
            }
            self.automation_gesture = Some(AutomationGesture {
                changed: true,
                ..gesture
            });
        }
        if primary_released
            && let Some(gesture) = self.automation_gesture
            && gesture.track_id == track_id
        {
            self.automation_gesture = None;
            if gesture.changed {
                self.sync_project();
            }
        }
        // Nested automation: another lane can drive this lane's base, depth,
        // rate, shape or envelope times, so the envelope is sampled from the
        // resolved values and the clip carries a badge per automated parameter.
        // Everything here is computed before the project is borrowed mutably;
        // a peak lane needs the engine's live follower, which the closure reads.
        let clip_left = (grid.grid_left
            + (track_start as f64 - grid.start_step as f64) as f32 * grid.step_width)
            .max(grid.grid_left);
        let clip_right = grid.grid_left
            + (track_start.saturating_add(track_length) as f64 - grid.start_step as f64) as f32
                * grid.step_width;
        let draw_columns = (((clip_right - clip_left) / grid.step_width.max(1.0) / 0.25).ceil()
            as usize)
            .clamp(2, 768);
        let lane_bpm = self.project.bpm;
        let lane_columns = lane_source_columns(
            &self.project,
            track_id,
            draw_columns,
            |index| {
                let x = clip_left + (clip_right - clip_left) * index as f32 / draw_columns as f32;
                grid.start_step as f64 + f64::from(x - grid.grid_left) / f64::from(grid.step_width)
            },
            lane_bpm,
            &|lane_id| self.peak_level(lane_id),
        );
        let lane_parameters = lane_automated_parameters(&self.project, track_id);
        // Peak lanes are the only source whose value cannot be prerendered, so
        // the engine's follower is read here and drawn as it moves.
        let lane_peak_level = self.peak_level(track_id);
        let project = &mut self.project;
        let lanes = &project.tracks;
        let bpm = project.bpm;
        let snap_ticks = project.snap_ticks;
        let Some(track) = project.automation_tracks.get_mut(track_id) else {
            return false;
        };
        painter.rect_filled(row, 0.0, egui::Color32::from_gray(25));
        let clip_start = grid.grid_left
            + (track.start_step as f64 - grid.start_step as f64) as f32 * grid.step_width;
        let clip_end_step = track.start_step.saturating_add(track.length_steps);
        let clip_end = grid.grid_left
            + (clip_end_step as f64 - grid.start_step as f64) as f32 * grid.step_width;
        // A clip that starts before the visible range must not paint over the
        // track header, and one entirely off-screen is skipped.
        if clip_end <= grid.grid_left {
            return false;
        }
        let clip_start = clip_start.max(grid.grid_left);
        let visible_span_steps = (clip_end - clip_start) / grid.step_width.max(1.0);
        let clip_rect = egui::Rect::from_min_max(
            egui::pos2(clip_start, row.top() + 4.0),
            egui::pos2(clip_end.max(clip_start + 2.0), row.bottom() - 4.0),
        );
        painter.rect_filled(clip_rect, 4.0, egui::Color32::from_rgb(49, 41, 62));
        // Title band: the only place a press moves the clip.
        painter.rect_filled(
            egui::Rect::from_min_max(
                clip_rect.min,
                egui::pos2(
                    clip_rect.right(),
                    (clip_rect.top() + AUTOMATION_CLIP_GRAB_HEIGHT).min(clip_rect.bottom()),
                ),
            ),
            4.0,
            egui::Color32::from_rgb(74, 59, 92),
        );
        painter.rect_stroke(
            clip_rect,
            4.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(143, 105, 176)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            egui::pos2(grid.timeline_left + 13.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            format!("Auto · {}", track.name),
            egui::FontId::proportional(12.0),
            egui::Color32::from_rgb(205, 155, 238),
        );
        // Automation of the lane itself: one pill per parameter that another
        // lane drives, right-aligned in the title band so the clip shows at a
        // glance that, say, its depth is being modulated.
        let mut badge_right = clip_rect.right() - 6.0;
        for label in &lane_parameters {
            let galley = painter.layout_no_wrap(
                format!("↺ {label}"),
                egui::FontId::proportional(9.5),
                egui::Color32::from_rgb(232, 205, 255),
            );
            let size = galley.size() + egui::vec2(6.0, 2.0);
            if badge_right - size.x < clip_rect.left() + 4.0 {
                break;
            }
            let badge = egui::Rect::from_min_size(
                egui::pos2(badge_right - size.x, clip_rect.top() + 1.0),
                size,
            );
            painter.rect_filled(badge, 2.0, egui::Color32::from_rgb(78, 54, 104));
            painter.galley(
                badge.min + egui::vec2(3.0, 1.0),
                galley,
                egui::Color32::WHITE,
            );
            badge_right = badge.left() - 4.0;
        }
        painter.line_segment(
            [
                egui::pos2(grid.grid_left, row.bottom()),
                egui::pos2(row.right(), row.bottom()),
            ],
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(91, 67, 110)),
        );
        // Right-clicking the lane offers duplication so one parameter can carry
        // several clips side by side, plus the transition chooser for the
        // segment under the pointer.
        let automation_menu = ContextMenu {
            items: vec![
                MenuItem::Action {
                    label: "Duplicate automation clip".to_owned(),
                    action: ContextAction::DuplicateAutomation(track_id),
                },
                MenuItem::Action {
                    label: "Delete automation clip".to_owned(),
                    action: ContextAction::DeleteAutomation(track_id),
                },
                MenuItem::Separator,
                MenuItem::Action {
                    label: "Plug-in settings".to_owned(),
                    action: ContextAction::OpenPluginSettings,
                },
            ],
        };
        let multiplier_points = track.modulation_points.clone();
        let curve_source = track.source.edits_points();
        // Relative lanes travel from -1 to 1 around the centre line; absolute
        // lanes use the usual 0 to 1 band.
        let value_floor = if track.source.is_relative() {
            -1.0
        } else {
            0.0
        };
        let value_y = |value: f32| {
            row.bottom()
                - 6.0
                - ((value.clamp(value_floor, 1.0) - value_floor) / (1.0 - value_floor))
                    * (row.height() - 12.0)
        };
        let value_at = |y: f32| {
            let fraction = (1.0 - (y - row.top() - 6.0) / (row.height() - 12.0)).clamp(0.0, 1.0);
            value_floor + fraction * (1.0 - value_floor)
        };
        let points = if curve_source {
            &mut track.points
        } else {
            &mut track.modulation_points
        };
        let point_position = |point: &AutomationPoint| {
            egui::pos2(
                grid.grid_left
                    + (point.step as f64 - grid.start_step as f64) as f32 * grid.step_width,
                value_y(point.value),
            )
        };
        if curve_source {
            let mut curve = Vec::new();
            // A relative lane reads against zero, so mark where zero sits.
            if value_floor < 0.0 {
                let zero_y = value_y(0.0);
                painter.line_segment(
                    [egui::pos2(clip_start, zero_y), egui::pos2(clip_end, zero_y)],
                    egui::Stroke::new(1.0_f32, egui::Color32::from_gray(78)),
                );
            }
            // Samples sit on a fixed quarter-step grid instead of pixel columns,
            // so zooming shifts the curve continuously instead of jittering.
            let sample_count = ((visible_span_steps / 0.25).ceil() as usize).clamp(2, 512);
            for index in 0..=sample_count {
                let x = clip_start + clip_rect.width() * index as f32 / sample_count as f32;
                let step = grid.start_step as f64
                    + f64::from(x - grid.grid_left) / f64::from(grid.step_width);
                let value = Project::evaluate_curve(points, step);
                curve.push(egui::pos2(x, value_y(value)));
            }
            painter.add(egui::Shape::line(
                curve,
                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(202, 158, 238)),
            ));
        } else if track.source.is_live() {
            // A peak lane has no stored shape: it follows the audio of a mixer
            // track, so the clip stays blank and carries a caption saying what
            // it listens to.
            let (track_id, post_fx) = match &track.source {
                project::AutomationSource::Peak {
                    channel_id,
                    post_fx,
                    ..
                } => (*channel_id, *post_fx),
                _ => (0, true),
            };
            let master_id = lanes.len();
            let channel_name = if track_id == master_id {
                "Master".to_owned()
            } else {
                lanes
                    .get(track_id)
                    .map_or_else(|| "a missing track".to_owned(), |lane| lane.name.clone())
            };
            let side = if post_fx { "post FX" } else { "pre FX" };
            // The follower level is live, so the clip fills as the track plays;
            // that is the one source that cannot be prerendered.
            let level = lane_peak_level.clamp(0.0, 1.0);
            let fill = egui::Rect::from_min_max(
                clip_rect.left_top(),
                egui::pos2(
                    clip_rect.left() + clip_rect.width() * level,
                    clip_rect.bottom(),
                ),
            );
            painter.rect_filled(
                fill,
                4.0,
                egui::Color32::from_rgba_unmultiplied(112, 208, 190, 46),
            );
            painter.line_segment(
                [
                    egui::pos2(fill.right(), clip_rect.top() + 2.0),
                    egui::pos2(fill.right(), clip_rect.bottom() - 2.0),
                ],
                egui::Stroke::new(1.6_f32, egui::Color32::from_rgb(112, 208, 190)),
            );
            // Follower motif: one loud bar and a decaying tail, so the blank
            // clip reads as an envelope and not as an empty lane.
            let motif_right = clip_rect.right() - 10.0;
            let bars = ((clip_rect.width() - 24.0) / 9.0).clamp(0.0, 9.0) as usize;
            for bar in 0..bars {
                let height = (clip_rect.height() - 26.0).max(6.0)
                    * (0.85 * 0.72_f32.powi(bar as i32)).max(0.06);
                let x = motif_right - bar as f32 * 9.0;
                painter.line_segment(
                    [
                        egui::pos2(x, clip_rect.center().y + height * 0.5),
                        egui::pos2(x, clip_rect.center().y - height * 0.5),
                    ],
                    egui::Stroke::new(
                        3.0_f32,
                        egui::Color32::from_rgba_unmultiplied(112, 208, 190, 120),
                    ),
                );
            }
            painter.text(
                egui::pos2(clip_rect.left() + 8.0, clip_rect.center().y),
                egui::Align2::LEFT_CENTER,
                format!("Peak of {channel_name} ({side}) · {:.0}%", level * 100.0),
                egui::FontId::proportional(11.5),
                egui::Color32::from_rgb(150, 220, 205),
            );
        } else {
            let source = track.source.clone();
            let target = track.target;
            let sample_count = draw_columns;
            let mut combined = Vec::with_capacity(sample_count + 1);
            let mut raw = Vec::with_capacity(sample_count + 1);
            let mut multiplier_curve = Vec::new();
            if !multiplier_points.is_empty() {
                multiplier_curve.reserve(sample_count + 1);
            }
            for index in 0..=sample_count {
                let x = clip_start + clip_rect.width() * index as f32 / sample_count as f32;
                let step = grid.start_step as f64
                    + f64::from(x - grid.grid_left) / f64::from(grid.step_width);
                // When another lane drives this lane's own parameters, draw the
                // resolved source so the envelope shows what will be heard.
                let column_source = match &lane_columns {
                    Some(sources) => sources[index].clone(),
                    None => source.clone(),
                };
                let value = automation_source_preview(
                    &column_source,
                    target,
                    lanes,
                    bpm,
                    track.start_step,
                    step,
                    lane_peak_level,
                )
                .clamp(0.0, 1.0);
                let multiplier = if multiplier_points.is_empty() {
                    1.0
                } else {
                    Project::evaluate_curve(&multiplier_points, step).clamp(0.0, 1.0)
                };
                let y_for = |level: f32| row.bottom() - 6.0 - level * (row.height() - 12.0);
                raw.push(egui::pos2(x, y_for(value * 0.85)));
                if !multiplier_points.is_empty() {
                    multiplier_curve.push(egui::pos2(x, y_for(multiplier)));
                }
                combined.push(egui::pos2(x, y_for(value * multiplier)));
            }
            painter.add(egui::Shape::line(
                raw,
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(70, 138, 126)),
            ));
            if !multiplier_curve.is_empty() {
                painter.add(egui::Shape::line(
                    multiplier_curve,
                    egui::Stroke::new(1.2_f32, egui::Color32::from_rgb(226, 190, 96)),
                ));
            }
            painter.add(egui::Shape::line(
                combined,
                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(112, 208, 190)),
            ));
        }
        let mut changed = false;
        let mut remove_point = None;
        // A live source (a track peak) has no stored shape, so its lane has no
        // points to drag or add.
        let live_source = track.source.is_live();
        let point_limit = if live_source { 0 } else { usize::MAX };
        for (point_id, point) in points.iter_mut().enumerate().take(point_limit) {
            let point_rect =
                egui::Rect::from_center_size(point_position(point), egui::vec2(12.0, 12.0));
            let response = ui.interact(
                point_rect,
                egui::Id::new(("automation_point", track_id, curve_source, point_id)),
                egui::Sense::click_and_drag(),
            );
            if response.dragged()
                && let Some(pointer) = context.input(|input| input.pointer.interact_pos())
            {
                let raw_step = (grid.start_step as f64
                    + f64::from(pointer.x - grid.grid_left) / f64::from(grid.step_width))
                .round()
                .max(0.0) as u64;
                point.step = snap_arrangement_step(raw_step, snap_ticks)
                    .max(track.start_step)
                    .min(clip_end_step.saturating_sub(1));
                point.value = value_at(pointer.y);
                changed = true;
            }
            response.context_menu(|ui| {
                if curve_source && point_id == 0 {
                    // The first point has no incoming segment, so there is no
                    // transition to shape; say so instead of offering a menu
                    // that changes nothing.
                    ui.label("First point: no incoming transition");
                    ui.separator();
                } else if curve_source {
                    ui.label(format!("Curve into this node (step {})", point.step));
                    for curve in [
                        project::AutomationCurve::Linear,
                        project::AutomationCurve::Smooth,
                        project::AutomationCurve::Bezier,
                        project::AutomationCurve::Curved,
                        project::AutomationCurve::Pulse,
                        project::AutomationCurve::Constant,
                        project::AutomationCurve::Steps,
                    ] {
                        if ui
                            .selectable_value(&mut point.curve, curve, format!("{curve:?}"))
                            .changed()
                        {
                            changed = true;
                        }
                    }
                    ui.separator();
                } else {
                    ui.label("Multiplier curve");
                    for curve in [
                        project::AutomationCurve::Linear,
                        project::AutomationCurve::Smooth,
                        project::AutomationCurve::Bezier,
                        project::AutomationCurve::Curved,
                        project::AutomationCurve::Pulse,
                        project::AutomationCurve::Constant,
                        project::AutomationCurve::Steps,
                    ] {
                        if ui
                            .selectable_value(&mut point.curve, curve, format!("{curve:?}"))
                            .changed()
                        {
                            changed = true;
                        }
                    }
                    ui.separator();
                }
                if ui.button("Delete point").clicked() {
                    remove_point = Some(point_id);
                    ui.close();
                }
            });
            // The marker comes from the shared point style, so every lane and
            // the piano roll draw points the same way.
            let style = reafl_widgets::shape::PointStyle {
                radius: 4.5,
                fill: if curve_source {
                    egui::Color32::from_rgb(229, 185, 251)
                } else {
                    egui::Color32::from_rgb(255, 218, 112)
                },
                ..Default::default()
            };
            style.paint(&painter, point_position(point));
        }
        if let Some(point_id) = remove_point {
            points.remove(point_id);
            changed = true;
        }
        points.sort_by_key(|point| point.step);
        let response = ui.interact(
            row,
            egui::Id::new(("automation_track", track_id)),
            egui::Sense::click(),
        );
        // The lane response is registered last, so it wins the pointer wherever
        // no point sits on top. Resolve the segment under a right-click now and
        // remember it, so the menu keeps pointing at the same transition while
        // it stays open.
        let lane_menu_key = egui::Id::new(("automation_lane_menu", track_id));
        if response.secondary_clicked()
            && let Some(pointer) = context.input(|input| input.pointer.interact_pos())
        {
            let step_at = grid.start_step as f64
                + f64::from(pointer.x - grid.grid_left) / f64::from(grid.step_width);
            let lane_points = if curve_source {
                &track.points
            } else {
                &track.modulation_points
            };
            let mut index = lane_points
                .iter()
                .position(|point| point.step as f64 >= step_at)
                .unwrap_or(lane_points.len().saturating_sub(1));
            if index == 0 {
                index = 1;
            }
            if index < lane_points.len() {
                context.data_mut(|data| data.insert_temp(lane_menu_key, index));
            }
        }
        let mut picked_action = None;
        response.context_menu(|ui| {
            let segment = ui.data(|data| data.get_temp::<usize>(lane_menu_key));
            let lane_points = if curve_source {
                &mut track.points
            } else {
                &mut track.modulation_points
            };
            if let Some(index) = segment
                && index > 0
                && index < lane_points.len()
            {
                let from_step = lane_points[index - 1].step;
                let to_step = lane_points[index].step;
                ui.label(if curve_source {
                    format!("Transition {from_step} → {to_step}")
                } else {
                    format!("Multiplier transition {from_step} → {to_step}")
                });
                for curve in [
                    project::AutomationCurve::Linear,
                    project::AutomationCurve::Smooth,
                    project::AutomationCurve::Bezier,
                    project::AutomationCurve::Curved,
                    project::AutomationCurve::Pulse,
                    project::AutomationCurve::Constant,
                    project::AutomationCurve::Steps,
                ] {
                    if ui
                        .selectable_value(
                            &mut lane_points[index].curve,
                            curve,
                            format!("{curve:?}"),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                }
                ui.separator();
            }
            if let Some(action) = automation_menu.show(ui) {
                picked_action = Some(action);
                ui.close();
            }
        });
        if let Some(action) = picked_action {
            self.apply_context_action(action);
            return false;
        }
        if response.clicked() && !live_source {
            self.windows.automation = true;
            if let Some(pointer) = context.input(|input| input.pointer.interact_pos()) {
                let raw_step = (grid.start_step as f64
                    + f64::from(pointer.x - grid.grid_left) / f64::from(grid.step_width))
                .round()
                .max(0.0) as u64;
                let point_step = snap_arrangement_step(raw_step, snap_ticks)
                    .max(track.start_step)
                    .min(clip_end_step.saturating_sub(1));
                let points = if curve_source {
                    &mut track.points
                } else {
                    &mut track.modulation_points
                };
                let near_existing = points.iter().any(|point| {
                    (point.step as f64 - point_step as f64).abs() * f64::from(grid.step_width) < 8.0
                });
                if !near_existing
                    && pointer.x >= clip_start
                    && pointer.x < clip_end
                    && pointer.x >= grid.grid_left
                {
                    let value = value_at(pointer.y);
                    points.push(AutomationPoint {
                        step: point_step,
                        value,
                        curve: AutomationCurve::Linear,
                    });
                    points.sort_by_key(|point| point.step);
                    changed = true;
                }
            }
        }
        if let project::AutomationSource::Perlin {
            seed,
            rate,
            octaves,
            ..
        } = &track.source
        {
            let position = self.audio.as_ref().map_or(0, |audio| {
                audio
                    .playhead_steps
                    .load(std::sync::atomic::Ordering::Relaxed)
            });
            let coordinate = match rate.unit {
                project::TimeUnit::Steps => {
                    (position.saturating_sub(track.start_step)) as f64
                        / f64::from(rate.amount.max(0.001))
                }
                project::TimeUnit::Milliseconds => {
                    (position.saturating_sub(track.start_step)) as f64 * 60.0
                        / f64::from(bpm.max(1.0))
                        / 4.0
                        / f64::from((rate.amount / 1_000.0).max(0.001))
                }
            };
            painter.text(
                egui::pos2(clip_rect.right() - 6.0, row.top() + 8.0),
                egui::Align2::RIGHT_TOP,
                format!(
                    "Perlin {:.3}",
                    project::perlin_noise_octaves(*seed, coordinate, *octaves)
                ),
                egui::FontId::proportional(9.0),
                egui::Color32::LIGHT_GRAY,
            );
        }
        changed
    }
}
