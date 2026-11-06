//! Arrangement layout maths and the previews drawn inside clips.

use crate::*;

pub(crate) fn arrangement_layout(project: &Project) -> Vec<RowSpan> {
    let mut spans = Vec::with_capacity(project.lanes.len() + project.automation_tracks.len());
    let mut top = 0.0;
    let push = |row: ArrangementRow, height: f32, top: &mut f32, spans: &mut Vec<RowSpan>| {
        spans.push(RowSpan {
            row,
            top: *top,
            height,
        });
        *top += height;
    };
    for lane_id in 0..project.lanes.len() {
        let lane = &project.lanes[lane_id];
        push(
            ArrangementRow::Lane(lane_id),
            lane.height,
            &mut top,
            &mut spans,
        );
        if lane.automation_collapsed {
            continue;
        }
        for (track_id, track) in project.automation_tracks.iter().enumerate() {
            if automation_owner_lane(project, track) == lane_id {
                push(
                    ArrangementRow::Automation(track_id),
                    AUTOMATION_ROW_HEIGHT,
                    &mut top,
                    &mut spans,
                );
            }
        }
    }
    for (track_id, track) in project.automation_tracks.iter().enumerate() {
        if automation_owner_lane(project, track) >= project.lanes.len() {
            push(
                ArrangementRow::Automation(track_id),
                AUTOMATION_ROW_HEIGHT,
                &mut top,
                &mut spans,
            );
        }
    }
    spans
}

/// Mixer lane an automation lane is grouped under. Master automation uses the
/// master index, so it sorts after every track.
pub(crate) fn automation_owner_lane(project: &Project, track: &AutomationTrack) -> usize {
    match track.target {
        ParameterTarget::Instrument { instrument_id, .. } => project
            .instruments
            .get(instrument_id)
            .map_or(0, |instrument| instrument.default_mixer_lane),
        ParameterTarget::InsertEffect { channel_id, .. } => channel_id,
        ParameterTarget::MixerChannel { channel_id, .. } => channel_id,
        // Lane-to-lane automation stays on the lane it belongs to.
        ParameterTarget::AutomationLane { .. } => project.lanes.len(),
        // Hosted plug-in parameters live on their insert slot, so group them
        // under the channel that owns that slot.
        ParameterTarget::PluginParameter { host_id, .. } => {
            let lane = ((host_id >> 16) & 0xff_ffff) as usize;
            lane.min(project.lanes.len())
        }
        ParameterTarget::Tempo => project.lanes.len(),
    }
}

/// Index of the row containing `offset` points below the grid top.
pub(crate) fn arrangement_row_at(spans: &[RowSpan], offset: f32) -> usize {
    for (index, span) in spans.iter().enumerate() {
        if offset < span.top + span.height {
            return index;
        }
    }
    spans.len().saturating_sub(1)
}

/// Draws what a clip contains: note blocks for pattern clips and a mini
/// waveform for audio clips. Clips with no preview read as empty slots, which
/// makes arrangement mistakes obvious at a glance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_clip_contents(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    clip: &ArrangementClip,
    patterns: &[project::Pattern],
    visible_start: u64,
    visible_end: u64,
    step_width: f32,
    pan_start: u64,
    grid_left: f32,
) {
    let inner = clip_rect.shrink2(egui::vec2(3.0, 4.0));
    match &clip.source {
        ClipSource::Pattern { pattern_id } => {
            let Some(pattern) = patterns.get(*pattern_id) else {
                return;
            };
            let pattern_length = pattern.length_steps.max(1);
            let pitch_range = pattern
                .notes
                .iter()
                .map(|note| note.pitch)
                .min()
                .zip(pattern.notes.iter().map(|note| note.pitch).max());
            let Some((low, high)) = pitch_range else {
                // Step-only patterns get one tick per active step.
                for hit in &pattern.step_hits {
                    // A sliced clip can start part-way into its pattern.
                    let offset = (hit.step + pattern_length - clip.offset_steps % pattern_length)
                        % pattern_length;
                    let step = clip.start_step + offset;
                    if step < visible_start || step >= visible_end {
                        continue;
                    }
                    let x = grid_left + step.saturating_sub(pan_start) as f32 * step_width;
                    painter.line_segment(
                        [
                            egui::pos2(x + step_width * 0.5, inner.bottom()),
                            egui::pos2(x + step_width * 0.5, inner.bottom() - inner.height() * 0.5),
                        ],
                        egui::Stroke::new(1.5_f32, egui::Color32::from_gray(220)),
                    );
                }
                return;
            };
            let span = u16::from(high.saturating_sub(low)).max(1);
            let note_height = (inner.height() * 0.18).clamp(3.0, 6.0);
            for note in &pattern.notes {
                // Repeats are drawn as extra blocks so a repeated note is
                // visible without opening the piano roll.
                for repeat in 0..=u64::from(note.repeat_count) {
                    let spacing = if note.repeat_ticks == 0 {
                        (note.duration_steps / 2).max(1)
                    } else {
                        u64::from(note.repeat_ticks) / 16
                    };
                    let source_step = note.start_step + repeat * spacing;
                    // Notes outside the pattern keep their own position: wrapping
                    // them made a shortened pattern look like it rearranged the
                    // notes.
                    if source_step >= pattern_length {
                        continue;
                    }
                    let offset = (source_step + pattern_length
                        - clip.offset_steps % pattern_length)
                        % pattern_length;
                    let absolute = clip.start_step + offset;
                    // Notes outside the clip or the visible window are skipped;
                    // clamping them to the window edge stacked every off-screen
                    // note on top of each other while panning.
                    if offset >= clip.length_steps
                        || absolute >= visible_end
                        || absolute.saturating_add(note.duration_steps.max(1)) <= visible_start
                    {
                        continue;
                    }
                    let x = grid_left + absolute.saturating_sub(pan_start) as f32 * step_width;
                    let width = (note.duration_steps.max(1) as f32 * step_width)
                        .min((inner.right() - x).max(2.0))
                        .max(2.0);
                    let normalized = f32::from(note.pitch.saturating_sub(low)) / f32::from(span);
                    let usable = (inner.height() - note_height).max(1.0);
                    let y = inner.bottom() - note_height - normalized * usable;
                    painter.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, note_height)),
                        1.5,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                    );
                }
            }
        }
        ClipSource::Audio { .. } => {
            // Audio clips show a repeating waveform motif; peak data would go
            // here once samples are analysed.
            let columns = (inner.width() / 3.0).floor().max(1.0) as usize;
            let center = inner.center().y;
            let seed = match &clip.source {
                ClipSource::Audio { path } => path.bytes().fold(7_u32, |acc, byte| {
                    acc.wrapping_mul(31).wrapping_add(u32::from(byte))
                }),
                ClipSource::Pattern { .. } => 7,
            };
            // A cheap hash keeps each clip's motif distinct instead of
            // repeating one sine everywhere.
            let mut noise = seed | 1;
            for column in 0..columns {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                let amplitude = (0.25 + (noise % 1000) as f32 / 1400.0) * inner.height() * 0.42;
                let x = inner.left() + column as f32 * 3.0 + 1.0;
                painter.line_segment(
                    [
                        egui::pos2(x, center - amplitude),
                        egui::pos2(x, center + amplitude),
                    ],
                    egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgba_unmultiplied(235, 232, 255, 150),
                    ),
                );
            }
        }
    }
}

pub(crate) fn automation_note_at(
    lanes: &[project::ArrangementLane],
    patterns: &[project::Pattern],
    instrument_id: usize,
    position: f64,
) -> Option<(f64, f64)> {
    let mut latest = None;
    for lane in lanes {
        for clip in &lane.clips {
            let ClipSource::Pattern { pattern_id } = clip.source else {
                continue;
            };
            let Some(pattern) = patterns.get(pattern_id) else {
                continue;
            };
            let pattern_length = pattern.length_steps.max(1);
            let clip_end = clip.start_step.saturating_add(clip.length_steps) as f64;
            let hits = pattern
                .step_hits
                .iter()
                .filter(|hit| hit.instrument_id == instrument_id)
                .map(|hit| (hit.step as f64, 1.0_f64));
            let notes = pattern
                .notes
                .iter()
                .filter(|note| note.instrument_id == instrument_id)
                .map(|note| {
                    (
                        note.start_tick() as f64 / 16.0,
                        note.duration_ticks() as f64 / 16.0,
                    )
                });
            for (offset, length) in hits.chain(notes) {
                let first = clip.start_step as f64 + offset;
                if position < first || first >= clip_end {
                    continue;
                }
                let cycle = ((position - first) / pattern_length as f64).floor();
                let start = first + cycle * pattern_length as f64;
                if start < clip_end && latest.is_none_or(|(last, _)| last < start) {
                    latest = Some((start, length));
                }
            }
        }
    }
    latest
}

pub(crate) fn automation_shape_value(shape: project::LfoShape, phase: f64) -> f32 {
    let cycle = phase.floor();
    let phase = phase.fract() as f32;
    match shape {
        project::LfoShape::Sine => (std::f32::consts::TAU * phase).sin(),
        project::LfoShape::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
        project::LfoShape::Saw => phase * 2.0 - 1.0,
        project::LfoShape::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        project::LfoShape::SampleAndHold => project::perlin_noise(0x7f4a_7c15, cycle + 0.5),
    }
}

pub(crate) fn automation_time_steps(value: project::TimeValue, bpm: f32) -> f64 {
    value.seconds(bpm) as f64 / (60.0 / f64::from(bpm.max(1.0)) / 4.0)
}

/// Names of the lane's own parameters that another lane drives.
///
/// The arrangement draws them as badges on the clip, so automation of a lane's
/// base, depth, rate, shape, sustain or octave count is visible where the lane
/// lives instead of only in the automation window.
///
/// # Parameters
///
/// - `project`: the project to inspect.
/// - `track_id`: the lane whose parameters are checked.
///
/// # Returns
///
/// One label per automated parameter, in picker order.
pub(crate) fn lane_automated_parameters(project: &Project, track_id: usize) -> Vec<&'static str> {
    use project::{AutomationLaneParameter as Parameter, ParameterTarget};
    [
        Parameter::Base,
        Parameter::Depth,
        Parameter::RateAmount,
        Parameter::Shape,
        Parameter::Sustain,
        Parameter::Octaves,
    ]
    .into_iter()
    .filter(|parameter| {
        project.has_automation(ParameterTarget::AutomationLane {
            track_id,
            parameter: *parameter,
        })
    })
    .map(|parameter| parameter.label())
    .collect()
}

/// Samples a lane's own base and depth for every drawing column.
///
/// When the lane's base or depth is itself automated, its envelope no longer
/// follows the stored values; sampling the driven values per column lets the
/// drawn curve show what the automation actually produces.
///
/// # Parameters
///
/// - `project`: the project to inspect.
/// - `track_id`: the lane being drawn.
/// - `columns`: number of samples to take, inclusive of both ends.
/// - `step_at`: maps a column index to a transport position in steps.
///
/// # Returns
///
/// `(base, depth)` per column, or `None` when neither is automated or the
/// source has no such values.
pub(crate) fn lane_source_columns(
    project: &Project,
    track_id: usize,
    columns: usize,
    step_at: impl Fn(usize) -> f64,
) -> Option<(Vec<f32>, Vec<f32>)> {
    use project::{AutomationLaneParameter as Parameter, ParameterTarget};
    let source = &project.automation_tracks.get(track_id)?.source;
    let base = source.base()?;
    let depth = source.depth()?;
    let base_target = ParameterTarget::AutomationLane {
        track_id,
        parameter: Parameter::Base,
    };
    let depth_target = ParameterTarget::AutomationLane {
        track_id,
        parameter: Parameter::Depth,
    };
    let base_driven = project.has_automation(base_target);
    let depth_driven = project.has_automation(depth_target);
    if !base_driven && !depth_driven {
        return None;
    }
    let mut bases = Vec::with_capacity(columns + 1);
    let mut depths = Vec::with_capacity(columns + 1);
    for index in 0..=columns {
        let step = step_at(index);
        bases.push(if base_driven {
            project.automated_value(base_target, base, step)
        } else {
            base
        });
        depths.push(if depth_driven {
            project.automated_value(depth_target, depth, step)
        } else {
            depth
        });
    }
    Some((bases, depths))
}

pub(crate) fn automation_source_preview(
    source: &project::AutomationSource,
    target: ParameterTarget,
    lanes: &[project::ArrangementLane],
    patterns: &[project::Pattern],
    bpm: f32,
    start_step: u64,
    position: f64,
) -> f32 {
    match source {
        project::AutomationSource::Curve | project::AutomationSource::RelativeCurve => 0.0,
        project::AutomationSource::MidiEnvelope {
            instrument_id,
            attack,
            decay,
            sustain,
            release,
            base,
            depth,
        } => {
            let Some((note_start, note_length)) =
                automation_note_at(lanes, patterns, *instrument_id, position)
            else {
                return *base;
            };
            let elapsed = position - note_start;
            let attack = automation_time_steps(*attack, bpm);
            let decay = automation_time_steps(*decay, bpm);
            let release = automation_time_steps(*release, bpm).max(0.001);
            let gate_end = note_length.max(0.0);
            let envelope = if elapsed < attack && attack > 0.0 {
                elapsed / attack
            } else if elapsed < attack + decay && decay > 0.0 {
                1.0 - (1.0 - sustain.clamp(0.0, 1.0) as f64) * ((elapsed - attack) / decay)
            } else if elapsed <= gate_end {
                sustain.clamp(0.0, 1.0) as f64
            } else {
                let release_start = if gate_end < attack {
                    gate_end / attack.max(0.001)
                } else if gate_end < attack + decay {
                    1.0 - (1.0 - sustain.clamp(0.0, 1.0) as f64)
                        * ((gate_end - attack) / decay.max(0.001))
                } else {
                    sustain.clamp(0.0, 1.0) as f64
                };
                release_start * (1.0 - (elapsed - gate_end) / release)
            };
            *base + envelope.clamp(0.0, 1.0) as f32 * *depth
        }
        project::AutomationSource::Lfo {
            shape,
            rate,
            retrigger,
            base,
            depth,
        } => {
            let phase_position = if *retrigger {
                automation_note_at(lanes, patterns, source_instrument_id(target), position)
                    .map_or(0.0, |(note_start, _)| position - note_start)
            } else {
                position
            };
            let phase = phase_position / automation_time_steps(*rate, bpm).max(0.001);
            *base + automation_shape_value(*shape, phase) * *depth
        }
        project::AutomationSource::SampleAndHold {
            seed,
            rate,
            base,
            depth,
        } => {
            let local = (position - start_step as f64).max(0.0);
            let phase = local / automation_time_steps(*rate, bpm).max(0.001);
            *base + audio::sample_and_hold(phase as f32, *seed) * *depth
        }
        project::AutomationSource::Perlin {
            seed,
            rate,
            octaves,
            base,
            depth,
        } => {
            let local = (position - start_step as f64).max(0.0);
            let phase = local / automation_time_steps(*rate, bpm).max(0.001);
            *base + project::perlin_noise_octaves(*seed, phase, *octaves) * *depth
        }
        // A peak lane has no stored shape: its value comes from the audio that
        // is playing, so the preview is the flat rest level and the lane draws a
        // placeholder clip instead of a curve.
        project::AutomationSource::Peak { base, .. } => *base,
    }
}

pub(crate) fn source_instrument_id(target: ParameterTarget) -> usize {
    match target {
        ParameterTarget::Instrument { instrument_id, .. } => instrument_id,
        _ => usize::MAX,
    }
}
