//! Arrangement layout maths and the previews drawn inside clips.

use crate::*;

pub(crate) fn arrangement_layout(project: &Project) -> Vec<RowSpan> {
    let mut spans = Vec::with_capacity(project.tracks.len() + project.automation_tracks.len());
    let mut top = 0.0;
    let push = |row: ArrangementRow, height: f32, top: &mut f32, spans: &mut Vec<RowSpan>| {
        spans.push(RowSpan {
            row,
            top: *top,
            height,
        });
        *top += height;
    };
    for lane_id in 0..project.tracks.len() {
        let lane = &project.tracks[lane_id];
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
        if automation_owner_lane(project, track) >= project.tracks.len() {
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

/// Mixer strip an automation lane is grouped under. Master automation uses the
/// master index, so it sorts after every track.
pub(crate) fn automation_owner_lane(project: &Project, track: &AutomationTrack) -> usize {
    match track.target {
        ParameterTarget::Track { track_id, .. } => track_id,
        ParameterTarget::InsertEffect { track_id, .. } => track_id,
        // Clip automation lives on the track that holds the clip.
        ParameterTarget::Clip { track_id, .. } => track_id,
        // Lane-to-lane automation stays on the lane it belongs to.
        ParameterTarget::AutomationLane { .. } => project.tracks.len(),
        // Hosted plug-in parameters live on their insert slot, so group them
        // under the track that owns that slot.
        ParameterTarget::PluginParameter { host_id, .. } => {
            let lane = ((host_id >> 16) & 0xff_ffff) as usize;
            lane.min(project.tracks.len())
        }
        ParameterTarget::Tempo => project.tracks.len(),
        ParameterTarget::SampleDefaults { .. } => project.tracks.len(),
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

/// Draws what a clip contains: note blocks for note clips and a mini waveform
/// for audio clips. Clips with no preview read as empty slots, which makes
/// arrangement mistakes obvious at a glance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_clip_contents(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    clip: &Clip,
    visible_start: u64,
    visible_end: u64,
    step_width: f32,
    pan_start: u64,
    grid_left: f32,
) {
    let inner = clip_rect.shrink2(egui::vec2(3.0, 4.0));
    if let Some(path) = &clip.audio_path {
        // Audio clips show a repeating waveform motif; peak data would go
        // here once samples are analysed.
        let columns = (inner.width() / 3.0).floor().max(1.0) as usize;
        let center = inner.center().y;
        // A cheap hash keeps each clip's motif distinct instead of repeating
        // one sine everywhere.
        let mut noise = path.bytes().fold(7_u32, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(u32::from(byte))
        }) | 1;
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
        return;
    }
    let pitch_range = clip
        .notes
        .iter()
        .map(|note| note.pitch)
        .min()
        .zip(clip.notes.iter().map(|note| note.pitch).max());
    let Some((low, high)) = pitch_range else {
        return;
    };
    let span = u16::from(high.saturating_sub(low)).max(1);
    let note_height = (inner.height() * 0.18).clamp(3.0, 6.0);
    for note in &clip.notes {
        // Repeats are drawn as extra blocks so a repeated note is visible
        // without opening the piano roll.
        for repeat in 0..=u64::from(note.repeat_count) {
            let spacing = if note.repeat_ticks == 0 {
                (note.duration_steps / 2).max(1)
            } else {
                u64::from(note.repeat_ticks) / 16
            };
            let source_step = note.start_step + repeat * spacing;
            // The clip's own offset hides the material before it.
            if source_step < clip.offset_steps {
                continue;
            }
            let offset = source_step - clip.offset_steps;
            let absolute = clip.start_step + offset;
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

/// Latest note of a track that starts at or before `position`.
///
/// # Parameters
///
/// - `track`: the track whose clips are scanned.
/// - `position`: transport position in steps.
///
/// # Returns
///
/// `(start, length)` of the latest note onset, in steps.
pub(crate) fn automation_note_at(track: &Track, position: f64) -> Option<(f64, f64)> {
    let mut latest: Option<(f64, f64)> = None;
    for clip in &track.clips {
        if clip.is_audio() {
            continue;
        }
        let clip_end = clip.end_step() as f64;
        for note in &clip.notes {
            let first = clip.start_step as f64 + note.start_step as f64;
            if position < first || first >= clip_end {
                continue;
            }
            if latest.is_none_or(|(last, _)| last < first) {
                latest = Some((first, note.duration_steps.max(1) as f64));
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
/// base, depth, rate, shape, sustain, octaves or envelope times is visible where
/// the lane lives instead of only in the automation window.
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
    use project::AutomationLaneParameter as Parameter;
    lane_driven_parameters(project, track_id)
        .into_iter()
        .map(Parameter::label)
        .collect()
}

/// Which of a lane's own parameters another lane drives.
///
/// # Parameters
///
/// - `project`: the project to inspect.
/// - `track_id`: the lane whose parameters are checked.
///
/// # Returns
///
/// Every driven parameter, in picker order.
pub(crate) fn lane_driven_parameters(
    project: &Project,
    track_id: usize,
) -> Vec<project::AutomationLaneParameter> {
    use project::{AutomationLaneParameter as Parameter, ParameterTarget};
    [
        Parameter::Base,
        Parameter::Depth,
        Parameter::RateAmount,
        Parameter::Shape,
        Parameter::Sustain,
        Parameter::Octaves,
        Parameter::Attack,
        Parameter::Decay,
        Parameter::Release,
    ]
    .into_iter()
    .filter(|parameter| {
        project.has_automation(ParameterTarget::AutomationLane {
            track_id,
            parameter: *parameter,
        })
    })
    .collect()
}

/// The value a parameter has at `position`, with every lane applied.
///
/// This is the UI's counterpart of the engine's evaluation: drawn curves replace
/// or offset the value, while every generated source (LFO, Perlin, sample & hold,
/// MIDI envelope, track peak) contributes a delta from its own centre. It exists
/// so the drawing and the knobs agree with what is heard for *any* combination of
/// source and target, including a lane whose rate is driven by a track peak.
///
/// # Parameters
///
/// - `project`: the project to inspect.
/// - `target`: the parameter to resolve.
/// - `position`: transport position in steps.
/// - `bpm`: tempo for the time-valued settings.
/// - `peak`: live follower level of a peak lane, indexed by lane.
///
/// # Returns
///
/// The resolved value, or `None` when the target no longer exists.
pub(crate) fn automated_value_at(
    project: &Project,
    target: ParameterTarget,
    position: f64,
    bpm: f32,
    peak: &dyn Fn(usize) -> f32,
) -> Option<f32> {
    automated_value_at_depth(project, target, position, bpm, peak, 0)
}

/// [`automated_value_at`] with a recursion guard for lane-on-lane automation.
fn automated_value_at_depth(
    project: &Project,
    target: ParameterTarget,
    position: f64,
    bpm: f32,
    peak: &dyn Fn(usize) -> f32,
    depth: u8,
) -> Option<f32> {
    let base = project.parameter_value(target)?;
    let base_normalized = target.normalize_value(base);
    let mut curve_total: Option<f32> = None;
    let mut relative_total = 0.0;
    let mut additive = 0.0;
    let mut touched = false;
    for (track_id, track) in project.automation_tracks.iter().enumerate() {
        if !track.enabled
            || track.target != target
            || position < track.start_step as f64
            || position >= track.start_step.saturating_add(track.length_steps) as f64
        {
            continue;
        }
        if track.source.edits_points() {
            if track.points.is_empty() {
                continue;
            }
            touched = true;
            let value = Project::evaluate_curve(&track.points, position);
            if track.source.is_relative() {
                relative_total += value;
                continue;
            }
            match &mut curve_total {
                Some(total) => *total += value - track.points[0].value,
                None => curve_total = Some(value),
            }
            continue;
        }
        let multiplier = if track.modulation_points.is_empty() {
            1.0
        } else {
            Project::evaluate_curve(&track.modulation_points, position).clamp(0.0, 1.0)
        };
        let resolved = resolved_lane_source(project, track_id, position, bpm, peak, depth)
            .unwrap_or_else(|| track.source.clone());
        let centre = resolved.base().unwrap_or(0.0);
        let peak_level = if matches!(resolved, project::AutomationSource::Peak { .. }) {
            peak(track_id)
        } else {
            0.0
        };
        let raw = automation_source_preview(
            &resolved,
            target,
            &project.tracks,
            bpm,
            track.start_step,
            position,
            peak_level,
        );
        additive += centre + (raw - centre) * multiplier - base_normalized;
        touched = true;
    }
    if !touched {
        return None;
    }
    Some(target.denormalize_value(
        (curve_total.unwrap_or(base_normalized) + relative_total + additive).clamp(0.0, 1.0),
    ))
}

/// A copy of a lane's source with its own automation applied at `position`.
///
/// A lane can automate another lane's base, depth, rate, shape, sustain, octave
/// count and envelope times. The engine folds those in before evaluating the
/// lane, so the UI has to do the same or the drawn envelope and the live values
/// would show the stored settings instead of what is heard.
///
/// # Parameters
///
/// - `project`: the project to inspect.
/// - `track_id`: the lane whose source is resolved.
/// - `position`: transport position in steps.
/// - `bpm`: tempo for the time-valued settings.
/// - `peak`: live follower level of a peak lane, indexed by lane.
/// - `depth`: recursion depth, so lane-on-lane automation terminates.
///
/// # Returns
///
/// The resolved source. It is a plain copy when nothing drives the lane.
pub(crate) fn resolved_lane_source(
    project: &Project,
    track_id: usize,
    position: f64,
    bpm: f32,
    peak: &dyn Fn(usize) -> f32,
    depth: u8,
) -> Option<project::AutomationSource> {
    use project::{
        AutomationLaneParameter as Parameter, AutomationSource, LfoShape, ParameterTarget,
    };
    let mut source = project.automation_tracks.get(track_id)?.source.clone();
    let driven = lane_driven_parameters(project, track_id);
    if driven.is_empty() || depth >= 3 {
        return Some(source);
    }
    let value = |parameter: Parameter, current: f32| {
        automated_value_at_depth(
            project,
            ParameterTarget::AutomationLane {
                track_id,
                parameter,
            },
            position,
            bpm,
            peak,
            depth + 1,
        )
        .unwrap_or(current)
    };
    let drives = |parameter: Parameter| driven.contains(&parameter);
    match &mut source {
        AutomationSource::MidiEnvelope {
            attack,
            decay,
            sustain,
            release,
            base,
            depth,
            ..
        } => {
            if drives(Parameter::Base) {
                *base = value(Parameter::Base, *base);
            }
            if drives(Parameter::Depth) {
                *depth = value(Parameter::Depth, *depth);
            }
            if drives(Parameter::Sustain) {
                *sustain = value(Parameter::Sustain, *sustain);
            }
            if drives(Parameter::Attack) {
                attack.amount = value(Parameter::Attack, attack.amount);
            }
            if drives(Parameter::Decay) {
                decay.amount = value(Parameter::Decay, decay.amount);
            }
            if drives(Parameter::Release) {
                release.amount = value(Parameter::Release, release.amount);
            }
        }
        AutomationSource::Lfo {
            shape,
            rate,
            base,
            depth,
            ..
        } => {
            if drives(Parameter::Base) {
                *base = value(Parameter::Base, *base);
            }
            if drives(Parameter::Depth) {
                *depth = value(Parameter::Depth, *depth);
            }
            if drives(Parameter::RateAmount) {
                rate.amount = value(Parameter::RateAmount, rate.amount);
            }
            if drives(Parameter::Shape) {
                *shape = LfoShape::from_index(
                    value(Parameter::Shape, shape.index() as f32).max(0.0) as u32,
                );
            }
        }
        AutomationSource::SampleAndHold {
            rate, base, depth, ..
        } => {
            if drives(Parameter::Base) {
                *base = value(Parameter::Base, *base);
            }
            if drives(Parameter::Depth) {
                *depth = value(Parameter::Depth, *depth);
            }
            if drives(Parameter::RateAmount) {
                rate.amount = value(Parameter::RateAmount, rate.amount);
            }
        }
        AutomationSource::Perlin {
            rate,
            octaves,
            base,
            depth,
            ..
        } => {
            if drives(Parameter::Base) {
                *base = value(Parameter::Base, *base);
            }
            if drives(Parameter::Depth) {
                *depth = value(Parameter::Depth, *depth);
            }
            if drives(Parameter::RateAmount) {
                rate.amount = value(Parameter::RateAmount, rate.amount);
            }
            if drives(Parameter::Octaves) {
                *octaves = value(Parameter::Octaves, *octaves as f32)
                    .round()
                    .clamp(1.0, 16.0) as u32;
            }
        }
        AutomationSource::Peak {
            attack,
            release,
            base,
            depth,
            ..
        } => {
            if drives(Parameter::Base) {
                *base = value(Parameter::Base, *base);
            }
            if drives(Parameter::Depth) {
                *depth = value(Parameter::Depth, *depth);
            }
            if drives(Parameter::Attack) {
                attack.amount = value(Parameter::Attack, attack.amount);
            }
            if drives(Parameter::Release) {
                release.amount = value(Parameter::Release, release.amount);
            }
        }
        AutomationSource::Curve | AutomationSource::RelativeCurve => {}
    }
    Some(source)
}

/// One resolved source per drawing column.
///
/// # Parameters
///
/// - `project`: the project to inspect.
/// - `track_id`: the lane being drawn.
/// - `columns`: number of samples to take, inclusive of both ends.
/// - `step_at`: maps a column index to a transport position in steps.
/// - `bpm`: tempo for the time-valued settings.
/// - `peak`: live follower level of a peak lane, indexed by lane.
///
/// # Returns
///
/// A source per column, or `None` when nothing drives the lane's own
/// parameters, in which case the caller can use the stored source directly.
pub(crate) fn lane_source_columns(
    project: &Project,
    track_id: usize,
    columns: usize,
    step_at: impl Fn(usize) -> f64,
    bpm: f32,
    peak: &dyn Fn(usize) -> f32,
) -> Option<Vec<project::AutomationSource>> {
    if lane_driven_parameters(project, track_id).is_empty() {
        return None;
    }
    let mut sources = Vec::with_capacity(columns + 1);
    for index in 0..=columns {
        sources.push(resolved_lane_source(
            project,
            track_id,
            step_at(index),
            bpm,
            peak,
            0,
        )?);
    }
    Some(sources)
}

/// The value a lane's source produces at a position, before it is compared with
/// the target's own value.
///
/// Everything except a track peak is derivable from the project, so the
/// arrangement can draw it ahead of playback. A peak lane is the exception: its
/// level only exists while audio runs, so the caller passes the follower value
/// the engine published for this frame.
///
/// # Parameters
///
/// - `source`: the lane source, already resolved against its own automation.
/// - `target`: the parameter the lane drives.
/// - `tracks`: tracks, for the note-driven sources.
/// - `bpm`: tempo used for the time-valued settings.
/// - `start_step`: start of the lane's clip.
/// - `position`: transport position in steps.
/// - `peak_level`: live follower level in 0..=1, used by peak lanes only.
///
/// # Returns
///
/// The source value in normalized 0..=1 units.
pub(crate) fn automation_source_preview(
    source: &project::AutomationSource,
    target: ParameterTarget,
    tracks: &[project::Track],
    bpm: f32,
    start_step: u64,
    position: f64,
    peak_level: f32,
) -> f32 {
    match source {
        project::AutomationSource::Curve | project::AutomationSource::RelativeCurve => 0.0,
        project::AutomationSource::MidiEnvelope {
            track_id,
            attack,
            decay,
            sustain,
            release,
            base,
            depth,
        } => {
            let Some((note_start, note_length)) = tracks
                .get(*track_id)
                .and_then(|track| automation_note_at(track, position))
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
                source_track_id(target)
                    .and_then(|track| tracks.get(track))
                    .and_then(|track| automation_note_at(track, position))
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
        // is playing, so the caller supplies the follower level the engine
        // published for this frame.
        project::AutomationSource::Peak { base, depth, .. } => {
            *base + peak_level.clamp(0.0, 1.0) * *depth
        }
    }
}

/// The track a target's notes come from, for the note-driven sources.
///
/// # Parameters
///
/// - `target`: the parameter address to inspect.
pub(crate) fn source_track_id(target: ParameterTarget) -> Option<usize> {
    match target {
        ParameterTarget::Track { track_id, .. } | ParameterTarget::Clip { track_id, .. } => {
            Some(track_id)
        }
        _ => None,
    }
}
