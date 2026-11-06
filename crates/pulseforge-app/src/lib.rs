//! The PulseForge editor: application state, windows and panels.
//!
//! This crate owns everything that needs to know about a loaded project: the
//! [`PulseForgeApp`] type, the arrangement, mixer, piano roll, channel rack and
//! automation views, the plug-in windows and the glue back to the audio engine.
//! Reusable drawing code lives in `pulseforge-widgets`, settings and background
//! jobs in their own crates, so this one is only the shell.
//!
//! [`run`] is the entry point used by the `pulseforge` binary.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, mpsc::SyncSender},
};

use clap::Parser;
use eframe::egui;
use midir::{Ignore, MidiInput, MidiInputConnection};
use pulseforge_core::{
    audio::{self, AudioCommand, AudioOutput},
    project,
    project::{
        ArrangementClip, AutomationCurve, AutomationPoint, AutomationTrack, ClipSource,
        DefaultSample, EffectParameter, InsertFxSlot, Instrument, InstrumentParameter,
        MIDI_CHANNEL_COUNT, ParameterTarget, PianoNote, Project, SignalWaveform, SynthKind, TimeSignature, disconnect_channel,
    },
};
use pulseforge_profiling::{
    Channel, PluginDebug, SystemMonitor, profile_scope, set_thread_channel,
    ui::{AudioDebug, DebugPanel, DebugWorld},
};
use pulseforge_settings::{cli, settings};
use pulseforge_tasks as tasks;
use pulseforge_widgets::{
    anim, automation_bridge, context_menu, control, effect_parameters, eq_curve, fader,
    fader_math, icons, knob, lane_slider, meter, mixer_strip, power, presets, shape, theme,
    time_edit, tooltip,
};

use anim::Animated;
use cli::Cli;
use context_menu::{ContextAction, ContextMenu, MenuItem};
use settings::{Chord, Settings, ShortcutAction};

mod actions;
mod geometry;
mod labels;
mod snap;
mod views;

use automation_bridge::*;
use control::ValueControl;
use effect_parameters::*;
use eq_curve::*;
use geometry::*;
use icons::*;
use knob::*;
use labels::*;
use lane_slider::*;
use mixer_strip::*;
use power::*;
use presets::*;
use snap::*;
use theme::*;
use time_edit::*;
use tooltip::*;

const BAR_WIDTH: f32 = 96.0;
const LANE_HEIGHT: f32 = 54.0;
const LANE_LABEL_WIDTH: f32 = 142.0;
const VISIBLE_PATTERN_STEPS: u64 = 32;
/// Width of one step-sequencer column. Fixed so every channel row lines up; the
/// rack scrolls horizontally when the window is narrower than the grid.
const STEP_COLUMN_WIDTH: f32 = 24.0;
const VISIBLE_PIANO_STEPS: u64 = 32;
const PIANO_MIN_STEP_WIDTH: f32 = 14.0;
const PIANO_MAX_STEP_WIDTH: f32 = 32.0;
const PIANO_MIN_ROW_HEIGHT: f32 = 12.0;
const PIANO_MAX_ROW_HEIGHT: f32 = 20.0;
const PIANO_MIN_PITCH: u8 = 48;
const PIANO_ROWS: usize = 37;
const VELOCITY_LANE_HEIGHT: f32 = 46.0;
const TOOL_CURSOR_SVG: &str = include_str!("../assets/tool_cursors.svg");
/// Global snap values offered by the toolbar and the snap shortcuts.
const SNAP_STEPS: &[u64] = &[1, 2, 4, 8, 16, 32, 64, 128];
/// Horizontal zoom limits for the arrangement and the editors.
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 3.0;
/// Height of the title band that moves an automation clip. Pressing anywhere
/// else inside the clip belongs to the curve points, so drawing and moving no
/// longer fight over the same gesture.
const AUTOMATION_CLIP_GRAB_HEIGHT: f32 = 8.0;

const AUTOMATED_TARGETS_KEY: &str = "pulseforge.automated_targets";
/// Keys of the automated target and live value snapshots the widgets read.
const AUTOMATED_VALUES_KEY: &str = "pulseforge.automated_values";

fn svg_cursor_points(kind: &str) -> Option<Vec<[f32; 2]>> {
    let marker = format!(r#"data-cursor="{kind}""#);
    let line = TOOL_CURSOR_SVG
        .lines()
        .find(|line| line.contains(&marker))?;
    let encoded = line.split_once("points=\"")?.1.split_once('"')?.0;
    let points: Vec<_> = encoded
        .split_whitespace()
        .filter_map(|point| {
            let (x, y) = point.split_once(',')?;
            Some([x.parse().ok()?, y.parse().ok()?])
        })
        .collect();
    (!points.is_empty()).then_some(points)
}

fn paint_svg_cursor(ui: &egui::Ui, kind: &str, position: egui::Pos2) {
    let Some(points) = svg_cursor_points(kind) else {
        return;
    };
    let vertices: Vec<_> = points
        .into_iter()
        .map(|[x, y]| position + egui::vec2(x * 1.15, y * 1.15))
        .collect();
    ui.painter().add(egui::Shape::convex_polygon(
        vertices,
        egui::Color32::from_rgba_unmultiplied(92, 228, 199, 205),
        egui::Stroke::new(1.0_f32, egui::Color32::WHITE),
    ));
}

/// Starts the PulseForge editor: parses the command line, installs the
/// profiler and runs the window until it closes.
///
/// # Errors
///
/// Returns the windowing error from `eframe` when the native window or the GL
/// context cannot be created.
pub fn run() -> eframe::Result<()> {
    let cli = Cli::parse();
    // The recorder has to exist before the audio thread starts, otherwise the
    // first callbacks would record into nothing and the flame graph would open
    // empty. Five seconds of history is a good default for interactive work.
    let profiler = pulseforge_profiling::install(5_000);
    profiler.set_enabled(true);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1024.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "PulseForge",
        options,
        Box::new(move |creation_context| {
            Ok(Box::new(PulseForgeApp::new(creation_context, &cli)))
        }),
    )
}

#[derive(Default)]
struct EditorWindows {
    channel_rack: bool,
    piano_roll: bool,
    mixer: bool,
    routing: bool,
    midi_routing: bool,
    automation: bool,
    render: bool,
    samples: bool,
    settings: bool,
}

/// Control lane shown underneath the piano-roll grid.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlLane {
    Velocity,
    Repeats,
}

/// Which settings page is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SettingsTab {
    #[default]
    Shortcuts,
    Plugins,
    Samples,
    Interface,
    Graphics,
    Theming,
    General,
}

/// Which list the sample browser panel shows.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SampleTab {
    #[default]
    Builtin,
    Folders,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PluginTarget {
    Instrument(usize),
    Effect { channel: usize, slot: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selection {
    Note {
        pattern: usize,
        instrument: usize,
        pitch: u8,
        start_step: u64,
    },
    Instrument(usize),
    Clip {
        lane: usize,
        clip: usize,
    },
    MixerChannel(usize),
    /// An automation lane row in the arrangement.
    Automation(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PianoTool {
    Pen,
    Select,
    Brush,
    Join,
    Cut,
}

#[derive(Clone, Copy)]
enum PianoGesture {
    Draw {
        pitch: u8,
        start_tick: u64,
        note_index: usize,
        default_duration_ticks: u64,
    },
    Move {
        note_index: usize,
        origin_tick: u64,
        origin_pitch: u8,
        pointer_tick: u64,
        pointer_pitch: u8,
    },
    Resize {
        note_index: usize,
        origin_duration_ticks: u64,
        pointer_tick: u64,
    },
    ResizeStart {
        note_index: usize,
        origin_start_tick: u64,
        origin_duration_ticks: u64,
        pointer_tick: u64,
    },
    Join {
        pitch: u8,
        start_step: u64,
        end_step: u64,
    },
}

#[derive(Clone, Copy)]
struct RackGesture {
    pattern_id: usize,
    instrument_id: usize,
    enabled: bool,
    last_step: u64,
}

#[derive(Clone)]
struct ArrangementGesture {
    lane: usize,
    start_step: u64,
    pointer_offset_steps: u64,
    source: ClipSource,
    resizing: bool,
    resizing_start: bool,
    resize_end_step: u64,
    changed: bool,
}

/// Drag state for moving an automation clip horizontally.
#[derive(Clone, Copy)]
struct AutomationGesture {
    track_id: usize,
    grab_step: u64,
    changed: bool,
}

#[derive(Clone, Copy)]
enum ArrangementRow {
    Lane(usize),
    Automation(usize),
}

#[derive(Clone, Copy)]
struct TimelineGrid {
    timeline_left: f32,
    grid_left: f32,
    start_step: u64,
    step_width: f32,
}

/// Height of one automation row inside a track group.
const AUTOMATION_ROW_HEIGHT: f32 = 30.0;

/// One arrangement row with its resolved vertical position.
#[derive(Clone, Copy)]
struct RowSpan {
    row: ArrangementRow,
    top: f32,
    height: f32,
}

/// Builds the arrangement's row layout.
///
/// Tracks own their height, automation rows use a fixed compact height, and a
/// collapsed track hides the automation rows grouped under it. Automation rows
/// stay directly below their track, which is what makes the grouping visible.
fn arrangement_layout(project: &Project) -> Vec<RowSpan> {
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
fn automation_owner_lane(project: &Project, track: &AutomationTrack) -> usize {
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
fn arrangement_row_at(spans: &[RowSpan], offset: f32) -> usize {
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
fn draw_clip_contents(
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
                    let offset = (hit.step + pattern_length
                        - clip.offset_steps % pattern_length)
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

fn automation_note_at(
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

fn automation_shape_value(shape: project::LfoShape, phase: f64) -> f32 {
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

fn automation_time_steps(value: project::TimeValue, bpm: f32) -> f64 {
    value.seconds(bpm) as f64 / (60.0 / f64::from(bpm.max(1.0)) / 4.0)
}

fn automation_source_preview(
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
    }
}

fn source_instrument_id(target: ParameterTarget) -> usize {
    match target {
        ParameterTarget::Instrument { instrument_id, .. } => instrument_id,
        _ => usize::MAX,
    }
}

struct PulseForgeApp {
    project: Project,
    audio: Option<AudioOutput>,
    audio_error: Option<String>,
    playing: bool,
    windows: EditorWindows,
    open_instrument_plugins: BTreeSet<usize>,
    open_effect_plugins: BTreeSet<(usize, usize)>,
    plugin_selector_target: Option<PluginTarget>,
    selected_pattern: usize,
    selected_pattern_playback: bool,
    audio_preview_pattern: Option<usize>,
    arrangement_loop: Option<(u64, u64)>,
    loop_drag_start: Option<u64>,
    /// True once the loop drag actually moved, so a plain click keeps the
    /// previous loop instead of creating a one-step one.
    loop_drag_moved: bool,
    loop_enabled: bool,
    audio_loop_range: Option<(u64, u64)>,
    selected_instrument: usize,
    selected_lane: usize,
    selected_mixer_channel: usize,
    automation_effect_mode: bool,
    automation_instrument_id: usize,
    automation_instrument_parameter: InstrumentParameter,
    automation_channel_id: usize,
    automation_slot_id: usize,
    automation_effect_parameter: EffectParameter,
    selection: Option<Selection>,
    /// Additional selected items, so shift/ctrl can build a multi-selection.
    extra_selection: Vec<Selection>,
    /// Anchor used for ctrl+click range selection.
    selection_anchor: Option<Selection>,
    /// Instrument being renamed inline in the channel rack.
    renaming_instrument: Option<usize>,
    /// Cached peak envelope for the sample shown in the sample-player window.
    sample_peaks: Option<(String, Vec<f32>)>,
    /// Cached bundle metadata for the hosted plug-in window.
    plugin_metadata: Option<(String, pulseforge_plugins::PluginMetadata)>,
    /// UI halves of loaded VST3/CLAP plug-ins, keyed by host id.
    hosted: std::collections::HashMap<u64, pulseforge_hosting::HostedUi>,
    /// Static data read once per load; querying it per frame is expensive for
    /// bundles with hundreds of parameters.
    hosted_meta: std::collections::HashMap<u64, HostedMeta>,
    /// Native child windows holding each hosted plug-in's editor.
    editor_windows: std::collections::HashMap<u64, pulseforge_hosting::editor::EditorWindow>,
    /// Last hosting error per slot, shown instead of a dead editor surface.
    hosted_errors: std::collections::HashMap<u64, String>,
    /// Editor currently attached, so it can be detached before its window dies.
    attached_editor: Option<u64>,
    /// Plug-in parameter last touched in a plug-in's own editor, with its name.
    last_tweaked: Option<(u64, String)>,
    /// Arrangement clip whose sample window is open.
    sample_clip_window: Option<(usize, usize)>,
    /// Size last requested from a plug-in editor, to avoid redundant resizes.
    editor_size_sent: std::collections::HashMap<u64, (u32, u32)>,
    /// HWND of the DAW window, captured once per frame.
    main_hwnd: isize,
    /// Pixels per point from the last frame, needed to place native windows.
    last_pixels_per_point: f32,
    /// Rubber-band origin for the select tool.
    box_select_start: Option<egui::Pos2>,
    /// Rubber-band origin (tick, pitch) for ctrl+drag in the piano roll.
    piano_box_origin: Option<(u64, u8)>,
    /// Modifier state sampled once per frame for selection helpers.
    mod_shift: bool,
    mod_ctrl: bool,
    arrangement_pan_steps: u64,
    arrangement_pan_lanes: usize,
    pattern_pan_steps: u64,
    piano_pan_steps: u64,
    piano_tool: PianoTool,
    piano_gesture: Option<PianoGesture>,
    rack_gesture: Option<RackGesture>,
    note_length_steps: u64,
    pattern_drag: Option<ClipSource>,
    /// Source placed by the arrangement pen and brush tools, mirroring the
    /// "last clicked item" behaviour of FL Studio.
    arrangement_source: Option<ClipSource>,
    arrangement_gesture: Option<ArrangementGesture>,
    /// True while the arrangement brush is painting a stroke.
    arrangement_painting: bool,
    /// Undo/redo snapshots of the whole project model.
    history: Vec<Project>,
    redo_history: Vec<Project>,
    /// Smoothed transport position for the animated playhead.
    playhead_display: f64,
    playhead_raw: u64,
    playhead_anchor: std::time::Instant,
    metronome: bool,
    /// Pending project snapshot plus the throttle timestamp for coalescing.
    project_dirty: bool,
    last_project_sync: std::time::Instant,
    settings: Settings,
    settings_path: String,
    settings_tab: SettingsTab,
    /// Cached external plug-ins discovered from the configured locations.
    external_plugins: Vec<pulseforge_plugins::ExternalPlugin>,
    plugin_scan_error: Option<String>,
    /// Chord being captured for a shortcut row, if any.
    capturing_shortcut: Option<ShortcutAction>,
    shortcut_draft: String,
    sample_tab: SampleTab,
    /// Files found under the configured sample folders.
    sample_files: Vec<String>,
    /// Animated visual state: clip lift, knob glow and panel fades.
    /// Track id being renamed, plus the in-progress text.
    renaming_track: Option<usize>,
    rename_buffer: String,
    /// Automation lane currently being dragged in the arrangement.
    automation_gesture: Option<AutomationGesture>,
    clip_lift: Animated,
    drag_ghost: Animated,
    inspector_fade: Animated,
    last_frame_seconds: f32,
    /// Set while a track height drag is in flight so it syncs once on release.
    track_resize_dirty: bool,
    /// Note repeat settings applied to newly drawn notes.
    /// Tool used by the clip arrangement (Join stays piano-roll only).
    arrangement_tool: PianoTool,
    /// Which control lane is shown under the piano roll.
    control_lane: ControlLane,
    /// Hides the control lane, giving the grid the full height.
    control_lane_collapsed: bool,
    /// Search filters for the settings and plug-in picker lists.
    settings_search: String,
    shortcut_search: String,
    plugin_search: String,
    /// Which tab a plug-in window shows.
    plugin_settings_tab: std::collections::BTreeSet<PluginTarget>,
    /// PluginTab::Settings ids are tracked here.
    /// Ruler drag used to select notes; it never touches the transport loop.
    piano_select_start: Option<u64>,
    piano_selection: Option<(u64, u64)>,
    note_repeat_count: u8,
    note_repeat_ticks: u8,
    note_repeat_decay: u8,
    routing_drag_from: Option<usize>,
    /// Route whose send knob is being dragged; latched so the knob survives
    /// the pointer leaving the cable.
    routing_gain_route: Option<(usize, usize)>,
    new_audio_path: String,
    new_audio_length_steps: u64,
    /// Text of the automation window's formula field.
    formula_text: String,
    /// Name typed into the mixer inspector's preset field.
    chain_preset_name: String,
    /// Presets found on disk, refreshed when the inspector saves or loads.
    chain_presets: Vec<String>,
    render_path: String,
    project_path: String,
    status: String,
    midi_ports: Vec<String>,
    midi_discovery_error: Option<String>,
    selected_midi_port: Option<usize>,
    midi_connection: Option<MidiInputConnection<()>>,
    midi_routes: Arc<Mutex<Vec<usize>>>,
    midi_callback_error: Arc<Mutex<Option<String>>>,
    /// Background jobs: plug-in scans, WAV decoding and project IO.
    tasks: tasks::Tasks,
    /// Per-slot timing counters for hosted VST3/CLAP plug-ins.
    plugin_debug: PluginDebug,
    /// The debug/profiler window plus its graph history.
    debug_panel: DebugPanel,
    /// Compact live numbers shown in the footer while the panel is open.
    debug_summary: String,
    /// CPU/RAM sampler running on its own thread.
    system_monitor: SystemMonitor,
}

impl PulseForgeApp {
    fn new(creation_context: &eframe::CreationContext<'_>, cli: &Cli) -> Self {
        // The settings path comes from the command line, so a portable install
        // (or a CI run) can point at its own configuration.
        let settings_path = cli.settings_path();
        let settings = Settings::load(&settings_path);
        apply_theme(&creation_context.egui_ctx, &settings.theme);
        let project = Project::default();
        // `--no-audio` keeps the engine from opening a device: hosted plug-ins
        // are then never instantiated, which is the safe way to open a project
        // that crashes a plug-in.
        let (audio, audio_error) = if cli.no_audio {
            (
                None,
                Some("Audio disabled (--no-audio); plug-ins stay unloaded.".to_owned()),
            )
        } else {
            match AudioOutput::start(&project) {
                Ok(output) => (Some(output), None),
                Err(error) => (None, Some(error)),
            }
        };
        let (midi_ports, midi_discovery_error) = match Self::query_midi_ports() {
            Ok(ports) => (ports, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        if let Some(audio) = &audio {
            let _ = audio.sender.try_send(AudioCommand::SetSmoothing {
                enabled: settings.parameter_smoothing,
                milliseconds: settings.smoothing_ms,
            });
        }
        let mut app = Self {
            midi_routes: Arc::new(Mutex::new(project.midi_routes.to_vec())),
            project,
            audio,
            audio_error,
            playing: false,
            windows: EditorWindows {
                channel_rack: true,
                ..Default::default()
            },
            open_instrument_plugins: BTreeSet::new(),
            open_effect_plugins: BTreeSet::new(),
            plugin_selector_target: None,
            selected_pattern: 0,
            selected_pattern_playback: false,
            audio_preview_pattern: None,
            arrangement_loop: None,
            loop_drag_start: None,
            loop_drag_moved: false,
            loop_enabled: false,
            audio_loop_range: None,
            selected_instrument: 0,
            selected_lane: 0,
            selected_mixer_channel: 0,
            automation_effect_mode: false,
            automation_instrument_id: 0,
            automation_instrument_parameter: InstrumentParameter::Gain,
            automation_channel_id: 0,
            automation_slot_id: 0,
            automation_effect_parameter: EffectParameter::Mix,
            selection: None,
            extra_selection: Vec::new(),
            selection_anchor: None,
            renaming_instrument: None,
            sample_peaks: None,
            plugin_metadata: None,
            hosted: std::collections::HashMap::new(),
            hosted_meta: std::collections::HashMap::new(),
            editor_windows: std::collections::HashMap::new(),
            hosted_errors: std::collections::HashMap::new(),
            attached_editor: None,
            last_tweaked: None,
            sample_clip_window: None,
            editor_size_sent: std::collections::HashMap::new(),
            main_hwnd: 0,
            last_pixels_per_point: 1.0,
            box_select_start: None,
            piano_box_origin: None,
            mod_shift: false,
            mod_ctrl: false,
            arrangement_pan_steps: 0,
            arrangement_pan_lanes: 0,
            pattern_pan_steps: 0,
            piano_pan_steps: 0,
            piano_tool: PianoTool::Pen,
            piano_gesture: None,
            rack_gesture: None,
            note_length_steps: 4,
            pattern_drag: None,
            arrangement_source: Some(ClipSource::Pattern { pattern_id: 0 }),
            arrangement_gesture: None,
            arrangement_painting: false,
            history: Vec::new(),
            redo_history: Vec::new(),
            playhead_display: 0.0,
            playhead_raw: 0,
            playhead_anchor: std::time::Instant::now(),
            metronome: false,
            project_dirty: false,
            last_project_sync: std::time::Instant::now(),
            settings,
            settings_path,
            settings_tab: SettingsTab::default(),
            external_plugins: Vec::new(),
            plugin_scan_error: None,
            capturing_shortcut: None,
            shortcut_draft: String::new(),
            sample_tab: SampleTab::default(),
            sample_files: Vec::new(),
            renaming_track: None,
            rename_buffer: String::new(),
            automation_gesture: None,
            clip_lift: Animated::new(0.0),
            drag_ghost: Animated::new(0.0),
            inspector_fade: Animated::new(0.0),
            last_frame_seconds: 1.0 / 60.0,
            track_resize_dirty: false,
            arrangement_tool: PianoTool::Pen,
            control_lane: ControlLane::Velocity,
            control_lane_collapsed: false,
            settings_search: String::new(),
            shortcut_search: String::new(),
            plugin_search: String::new(),
            plugin_settings_tab: std::collections::BTreeSet::new(),
            piano_select_start: None,
            piano_selection: None,
            note_repeat_count: 0,
            note_repeat_ticks: 0,
            note_repeat_decay: 8,
            routing_drag_from: None,
            routing_gain_route: None,
            new_audio_path: String::new(),
            new_audio_length_steps: 64,
            render_path: "pulseforge.wav".to_owned(),
            formula_text: "0.5 + 0.4 * sin(2 * pi * x)".to_owned(),
            chain_preset_name: "My chain".to_owned(),
            chain_presets: list_chain_presets(),
            project_path: "session.json".to_owned(),
            status: "Ready · drag a pattern or audio clip onto the timeline".to_owned(),
            midi_ports,
            midi_discovery_error,
            selected_midi_port: None,
            midi_connection: None,
            midi_callback_error: Arc::new(Mutex::new(None)),
            tasks: tasks::Tasks::new(),
            plugin_debug: PluginDebug::default(),
            debug_panel: DebugPanel::new(cli.debug),
            debug_summary: String::new(),
            system_monitor: SystemMonitor::start(),
        };
        // Start-up work runs off the UI thread: a large plug-in folder or
        // sample library no longer delays the first frame.
        app.rescan_plugins();
        app.refresh_sample_files();
        if let Some(path) = cli.project_path() {
            app.project_path = path;
            if std::path::Path::new(&app.project_path).exists() {
                app.load_project();
            } else {
                app.status = format!("Project {} not found; starting empty.", app.project_path);
            }
        }
        app
    }

    /// Queues a plug-in folder scan on a worker thread. The status line is set
    /// immediately and the result arrives through [`Self::apply_task`].
    fn rescan_plugins(&mut self) {
        let roots = self.settings.plugin_roots();
        let settings_missing = roots.is_empty();
        self.status = "Scanning plug-in folders…".to_owned();
        self.tasks.scan_plugins(roots, settings_missing);
    }

    fn save_settings(&mut self) {
        match self.settings.save(&self.settings_path) {
            Ok(()) => self.status = format!("Settings saved to {}.", self.settings_path),
            Err(error) => self.status = error,
        }
    }

    /// Pushes the settings that affect the audio engine.
    fn apply_audio_settings(&mut self) {
        self.send_audio(AudioCommand::SetSmoothing {
            enabled: self.settings.parameter_smoothing,
            milliseconds: self.settings.smoothing_ms,
        });
    }

    /// Refreshes the sample browser file list from the configured folders.
    /// Directory walking happens on a worker thread so a network share or a
    /// huge library cannot stall a frame.
    fn refresh_sample_files(&mut self) {
        let roots = self.settings.sample_paths.clone();
        self.status = "Scanning sample folders…".to_owned();
        self.tasks.scan_samples(roots);
    }

    /// Auditions a sample through the master bus.
    fn preview_sample(&mut self, path: &str) {
        self.status = format!("Loading {path}…");
        self.tasks
            .decode_sample(path.to_owned(), tasks::SampleUse::Preview);
    }

    /// Maps a chord key name to its egui key.
    fn egui_key(name: &str) -> Option<egui::Key> {
        use egui::Key;
        Some(match name {
            "Space" => Key::Space,
            "Enter" => Key::Enter,
            "Tab" => Key::Tab,
            "Escape" => Key::Escape,
            "Delete" => Key::Delete,
            "Home" => Key::Home,
            "Comma" => Key::Comma,
            "Period" => Key::Period,
            "Minus" => Key::Minus,
            "F2" => Key::F2,
            "F5" => Key::F5,
            "F6" => Key::F6,
            "F7" => Key::F7,
            "F8" => Key::F8,
            "F9" => Key::F9,
            "F10" => Key::F10,
            "F11" => Key::F11,
            "F12" => Key::F12,
            "A" => Key::A,
            "B" => Key::B,
            "C" => Key::C,
            "D" => Key::D,
            "E" => Key::E,
            "F" => Key::F,
            "G" => Key::G,
            "H" => Key::H,
            "I" => Key::I,
            "J" => Key::J,
            "K" => Key::K,
            "L" => Key::L,
            "M" => Key::M,
            "N" => Key::N,
            "O" => Key::O,
            "P" => Key::P,
            "Q" => Key::Q,
            "R" => Key::R,
            "S" => Key::S,
            "T" => Key::T,
            "U" => Key::U,
            "V" => Key::V,
            "W" => Key::W,
            "X" => Key::X,
            "Y" => Key::Y,
            "Z" => Key::Z,
            "0" => Key::Num0,
            "1" => Key::Num1,
            "2" => Key::Num2,
            "3" => Key::Num3,
            "4" => Key::Num4,
            "5" => Key::Num5,
            "6" => Key::Num6,
            "7" => Key::Num7,
            "8" => Key::Num8,
            "9" => Key::Num9,
            _ => return None,
        })
    }

    /// Turns the event stream into actions using the configured key map, so
    /// editing a binding in Settings immediately changes behaviour.
    fn pressed_actions(&self, context: &egui::Context) -> Vec<ShortcutAction> {
        let mut pressed = Vec::new();
        for binding in &self.settings.shortcuts {
            if !binding.enabled || binding.keys.trim().is_empty() {
                continue;
            }
            let chord = Chord::parse(&binding.keys);
            let Some(key) = chord.key.and_then(Self::egui_key) else {
                continue;
            };
            let matched = context.input(|input| {
                input.key_pressed(key)
                    && input.modifiers.ctrl == chord.ctrl
                    && input.modifiers.shift == chord.shift
                    && input.modifiers.alt == chord.alt
            });
            if matched {
                pressed.push(binding.action);
            }
        }
        pressed
    }

    /// Executes one command. Every shortcut and toolbar button routes here, so
    /// behaviour stays identical however the command is triggered.
    fn perform(&mut self, action: ShortcutAction) {
        match action {
            ShortcutAction::TransportToggle => {
                self.playing = !self.playing;
                self.send_audio(AudioCommand::SetPlaying(self.playing));
            }
            ShortcutAction::TransportStop => {
                self.playing = false;
                // Rewind first so the engine's position is already reset when
                // the next play command arrives.
                self.send_audio(AudioCommand::Rewind);
                self.send_audio(AudioCommand::SetPlaying(false));
                self.playhead_display = 0.0;
                self.playhead_raw = 0;
            }
            ShortcutAction::ToggleLoop => {
                if self.arrangement_loop.is_some() {
                    self.loop_enabled = !self.loop_enabled;
                } else {
                    self.status = "Drag above the arrangement to create a loop first.".to_owned();
                }
            }
            ShortcutAction::TogglePatternPlayback => {
                self.selected_pattern_playback = !self.selected_pattern_playback;
            }
            ShortcutAction::ToggleMetronome => {
                self.metronome = !self.metronome;
                self.send_audio(AudioCommand::SetMetronome(self.metronome));
            }
            ShortcutAction::WindowChannelRack => {
                self.windows.channel_rack = !self.windows.channel_rack;
            }
            ShortcutAction::WindowPianoRoll => {
                self.windows.piano_roll = !self.windows.piano_roll;
            }
            ShortcutAction::WindowMixer => self.windows.mixer = !self.windows.mixer,
            ShortcutAction::WindowRouting => self.windows.routing = !self.windows.routing,
            ShortcutAction::WindowAutomation => self.windows.automation = !self.windows.automation,
            ShortcutAction::WindowMidi => self.windows.midi_routing = !self.windows.midi_routing,
            ShortcutAction::WindowSamples => self.windows.samples = !self.windows.samples,
            ShortcutAction::WindowSettings => self.windows.settings = !self.windows.settings,
            ShortcutAction::WindowRender => self.windows.render = !self.windows.render,
            ShortcutAction::WindowDebug => self.debug_panel.toggle(),
            ShortcutAction::ToolPen => self.set_tool(PianoTool::Pen),
            ShortcutAction::ToolSelect => self.set_tool(PianoTool::Select),
            ShortcutAction::ToolBrush => self.set_tool(PianoTool::Brush),
            ShortcutAction::ToolJoin => self.set_tool(PianoTool::Join),
            ShortcutAction::ToolCut => self.set_tool(PianoTool::Cut),
            ShortcutAction::SnapUp => self.step_snap(1),
            ShortcutAction::SnapDown => self.step_snap(-1),
            ShortcutAction::ControlVelocity => self.control_lane = ControlLane::Velocity,
            ShortcutAction::ControlRepeats => self.control_lane = ControlLane::Repeats,
            ShortcutAction::NoteUp => self.shift_selected_notes(1, 0, false),
            ShortcutAction::NoteDown => self.shift_selected_notes(-1, 0, false),
            ShortcutAction::NoteOctaveUp => self.shift_selected_notes(12, 0, false),
            ShortcutAction::NoteOctaveDown => self.shift_selected_notes(-12, 0, false),
            ShortcutAction::NoteLeft => self.shift_selected_notes(0, -1, true),
            ShortcutAction::NoteRight => self.shift_selected_notes(0, 1, true),
            ShortcutAction::Undo => self.undo(),
            ShortcutAction::Redo => self.redo(),
            ShortcutAction::SelectAll => self.select_all_notes(),
            ShortcutAction::DuplicateClip => self.duplicate_selected_clip(),
            ShortcutAction::DeleteSelection => self.delete_selected_clip(),
            ShortcutAction::RenameTrack => self.begin_rename_track(),
            ShortcutAction::RouteToFreeInsert => {
                self.route_instrument_to_next_insert(self.selected_instrument);
            }
            ShortcutAction::ClearSelection => self.selection = None,
            ShortcutAction::ResetPan => {
                self.arrangement_pan_steps = 0;
                self.arrangement_pan_lanes = 0;
                self.pattern_pan_steps = 0;
                self.piano_pan_steps = 0;
            }
            ShortcutAction::SaveProject => self.save_project(),
            ShortcutAction::LoadProject => self.load_project(),
        }
    }

    /// Peak envelope for `path`, computed once and cached per path.
    fn peaks_for(&mut self, path: &str, buckets: usize) -> Option<Vec<f32>> {
        if let Some((cached_path, peaks)) = &self.sample_peaks
            && cached_path == path
            && peaks.len() == buckets
        {
            return Some(peaks.clone());
        }
        let sample = audio::read_wav(path).ok()?;
        let frames = sample.frames.len();
        if frames == 0 {
            return None;
        }
        let width = buckets.max(1);
        let mut peaks = vec![0.0_f32; width];
        for (index, peak) in peaks.iter_mut().enumerate() {
            let start = index * frames / width;
            let end = ((index + 1) * frames / width).max(start + 1).min(frames);
            *peak = sample.frames[start..end]
                .iter()
                .map(|frame| frame[0].abs().max(frame[1].abs()))
                .fold(0.0_f32, f32::max);
        }
        self.sample_peaks = Some((path.to_owned(), peaks.clone()));
        Some(peaks)
    }

    /// Draws a cached waveform for an instrument's sample.
    fn draw_sample_waveform(
        &mut self,
        painter: egui::Painter,
        rect: egui::Rect,
        instrument_id: usize,
    ) {
        let path = self
            .project
            .instruments
            .get(instrument_id)
            .map(|instrument| instrument.sample_path.clone())
            .unwrap_or_default();
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(20));
        if path.trim().is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No sample loaded",
                egui::FontId::proportional(11.0),
                egui::Color32::from_gray(120),
            );
            return;
        }
        let buckets = (rect.width() / 2.0).max(16.0) as usize;
        let Some(peaks) = self.peaks_for(&path, buckets) else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Sample could not be read",
                egui::FontId::proportional(11.0),
                egui::Color32::LIGHT_RED,
            );
            return;
        };
        let color = self.instrument_color(instrument_id);
        let center = rect.center().y;
        for (index, peak) in peaks.iter().enumerate() {
            let x = rect.left() + index as f32 * 2.0;
            let half = (peak * rect.height() * 0.45).max(0.5);
            painter.line_segment(
                [egui::pos2(x, center - half), egui::pos2(x, center + half)],
                egui::Stroke::new(1.0_f32, color),
            );
        }
    }

    fn selected_note_count(&self) -> usize {
        let primary = usize::from(matches!(self.selection, Some(Selection::Note { .. })));
        primary
            + self
                .extra_selection
                .iter()
                .filter(|item| matches!(item, Selection::Note { .. }))
                .count()
    }

    fn select_all_notes(&mut self) {
        let all = self.note_order();
        let mut iter = all.into_iter();
        self.selection = iter.next();
        self.extra_selection = iter.collect();
        self.selection_anchor = self.selection;
    }

    /// Step count shown in the piano roll: covers the pattern length and every
    /// note, so shortening a pattern never hides notes that sit beyond it.
    fn piano_visible_steps(&self) -> u64 {
        let pattern = self.project.patterns.get(self.selected_pattern);
        let pattern_length =
            pattern.map_or(VISIBLE_PIANO_STEPS, |pattern| pattern.length_steps);
        let note_end = pattern.map_or(0, |pattern| {
            pattern
                .notes
                .iter()
                .map(|note| note.start_step.saturating_add(note.duration_steps))
                .max()
                .unwrap_or(0)
        });
        pattern_length
            .max(note_end.saturating_add(1))
            .clamp(VISIBLE_PIANO_STEPS, 4096)
    }

    /// Window title for a plug-in window: uses the hosted plug-in name when the
    /// instrument is backed by an external bundle.
    fn instrument_window_title(&self, instrument_id: usize) -> String {
        let Some(instrument) = self.project.instruments.get(instrument_id) else {
            return "Plug-in".to_owned();
        };
        if instrument.synth_kind == SynthKind::External && !instrument.plugin_path.is_empty() {
            return format!("{} \u{00B7} Hosted plug-in", instrument.name);
        }
        format!(
            "{} \u{00B7} {}",
            instrument.name,
            synth_kind_label(instrument.synth_kind)
        )
    }

    /// Bundle metadata for `path`, read once per path.
    fn metadata_for(&mut self, path: &str) -> pulseforge_plugins::PluginMetadata {
        if let Some((cached_path, metadata)) = &self.plugin_metadata
            && cached_path == path
        {
            return metadata.clone();
        }
        let metadata = pulseforge_plugins::read_metadata(path);
        self.plugin_metadata = Some((path.to_owned(), metadata.clone()));
        metadata
    }

    /// Loads a bundle and hands its audio half to the engine. The editor half
    /// stays here, because both formats require their GUI on the UI thread.
    fn load_host(&mut self, id: u64, path: &str, instrument: bool) {
        self.unload_host(id);
        let Some(audio) = self.audio.as_ref() else {
            self.hosted_errors.insert(
                id,
                "No audio device is running, so the plug-in cannot be hosted.".to_owned(),
            );
            return;
        };
        let sample_rate = f64::from(audio.sample_rate);
        // The counters are created here and travel to the audio thread inside
        // the load command, so block times can be attributed to this slot
        // without any locking on the audio path.
        let counters = self.plugin_debug.register(
            id,
            std::path::Path::new(path)
                .file_stem()
                .map_or_else(|| path.to_owned(), |stem| stem.to_string_lossy().into_owned()),
            path,
            if instrument {
                pulseforge_profiling::PluginKind::Instrument
            } else {
                pulseforge_profiling::PluginKind::Effect
            },
            pulseforge_core::hosted::HOST_BLOCK as u32,
            audio.sample_rate,
        );
        match pulseforge_hosting::load(
            path,
            instrument,
            sample_rate,
            pulseforge_core::hosted::HOST_BLOCK,
        ) {
            Ok((mut ui, audio_half)) => {
                let _ = audio.sender.try_send(AudioCommand::LoadHosted {
                    id,
                    audio: Box::new(audio_half),
                    instrument,
                    counters: Some(Arc::clone(&counters)),
                });
                // Restore the state saved with the project, if any.
                if let Some(state) = self.stored_plugin_state(id)
                    && !state.is_empty()
                {
                    ui.load_state(&state);
                }
                let meta = HostedMeta {
                    info: ui.info().clone(),
                    parameters: ui.parameters(),
                    editor_size: ui.editor_size().unwrap_or((640, 420)),
                };
                self.hosted_meta.insert(id, meta);
                self.hosted.insert(id, ui);
            }
            Err(error) => {
                // Loading failures are the most common "why is my plug-in
                // silent" cause, so they are also kept for the debug panel.
                counters.record_error(&error.to_string());
                self.hosted_errors.insert(id, error.to_string());
            }
        }
    }

    /// Plug-in state stored in the project for a host id, if any.
    fn stored_plugin_state(&self, id: u64) -> Option<Vec<u8>> {
        for (instrument_id, instrument) in self.project.instruments.iter().enumerate() {
            if pulseforge_core::hosted::instrument_host_id(instrument_id) == id {
                return Some(instrument.plugin_state.clone());
            }
        }
        let lane_count = self.project.lanes.len();
        for channel in 0..=lane_count {
            let slots = if channel == lane_count {
                &self.project.master.inserts
            } else {
                &self.project.lanes[channel].inserts
            };
            for (slot, effect) in slots.iter().enumerate() {
                if pulseforge_core::hosted::effect_host_id(channel, slot) == id {
                    return Some(effect.plugin_state.clone());
                }
            }
        }
        None
    }

    fn unload_host(&mut self, id: u64) {
        if self.attached_editor == Some(id) {
            self.detach_editor();
        }
        self.plugin_debug.unregister(id);
        if self.hosted.remove(&id).is_some()
            && let Some(audio) = self.audio.as_ref()
        {
            let _ = audio.sender.try_send(AudioCommand::UnloadHosted { id });
        }
        self.editor_windows.remove(&id);
        self.hosted_errors.remove(&id);
        self.hosted_meta.remove(&id);
    }

    /// Mirrors the plug-in's own editor changes into the "last tweaked"
    /// parameter, which the wrapper offers to automate.
    fn poll_last_tweaked(&mut self) {
        let Some(host_id) = self.attached_editor else {
            return;
        };
        let Some((parameter_id, _value)) = self
            .hosted
            .get_mut(&host_id)
            .and_then(pulseforge_hosting::HostedUi::take_last_tweaked)
        else {
            return;
        };
        let name = self
            .hosted_meta
            .get(&host_id)
            .and_then(|meta| {
                meta.parameters
                    .iter()
                    .find(|parameter| parameter.id == parameter_id)
            })
            .map(|parameter| parameter.name.clone())
            .unwrap_or_else(|| format!("Parameter {parameter_id}"));
        self.last_tweaked = Some((host_id, name));
    }

    /// Adds an automation lane for any target, styling it from the current
    /// value so the first point matches what the user sees.
    fn create_automation_for_target(&mut self, target: ParameterTarget) {
        self.push_history();
        let label = parameter_target_label(&self.project, target);
        let base = self
            .project
            .parameter_value(target)
            .map_or(0.5, |value| target.normalize_value(value));
        let track_id = self.project.add_automation_track(AutomationTrack {
            name: label,
            target,
            points: vec![AutomationPoint {
                step: 0,
                value: base,
                curve: AutomationCurve::Linear,
            }],
            ..AutomationTrack::default()
        });
        self.status = format!("Automation lane created for track {}.", track_id + 1);
        // Show the new lane straight away, so the action has a visible result.
        self.windows.automation = true;
        self.sync_project();
    }

    /// Creates an automation lane that drives a hosted plug-in parameter.
    fn create_plugin_automation(&mut self, host_id: u64, name: &str) {
        let parameter_id = self
            .hosted_meta
            .get(&host_id)
            .and_then(|meta| meta.parameters.iter().find(|p| p.name == name))
            .map(|parameter| parameter.id)
            .unwrap_or(0);
        self.push_history();
        let track_id = self.project.add_automation_track(AutomationTrack {
            name: name.to_owned(),
            target: ParameterTarget::PluginParameter {
                host_id,
                parameter_id,
            },
            ..AutomationTrack::default()
        });
        if let Some(track) = self.project.automation_tracks.get_mut(track_id) {
            track.name = name.to_owned();
        }
        self.status = format!("Automation lane created for {name}.");
        self.sync_project();
    }

    /// Keeps a plug-in editor in step with the window rectangle it occupies.
    fn resize_editor_to(&mut self, host_id: u64, rect: egui::Rect) {
        if self.attached_editor != Some(host_id) {
            return;
        }
        let scale = self.last_pixels_per_point.max(0.5);
        let wanted = (
            (rect.width() * scale).round().max(1.0) as u32,
            (rect.height() * scale).round().max(1.0) as u32,
        );
        if self.editor_size_sent.get(&host_id) == Some(&wanted) {
            return;
        }
        self.editor_size_sent.insert(host_id, wanted);
        if let Some(hosted) = self.hosted.get_mut(&host_id) {
            let _ = hosted.resize_editor(wanted.0, wanted.1);
        }
    }

    /// Detaches the live editor (the plug-in keeps running headless).
    fn detach_editor(&mut self) {
        if let Some(id) = self.attached_editor.take() {
            if let Some(ui) = self.hosted.get_mut(&id) {
                ui.close_editor();
            }
            if let Some(window) = self.editor_windows.get_mut(&id) {
                window.set_visible(false);
            }
        }
    }

    /// Keeps the native editor window in sync with the egui wrapper, attaching
    /// it the first time the wrapper is laid out.
    fn sync_editor_window(&mut self, host_id: u64, rect: egui::Rect) {
        let scale = self.last_pixels_per_point.max(0.5);
        let x = (rect.min.x * scale).round() as i32;
        let y = (rect.min.y * scale).round() as i32;
        let width = (rect.width() * scale).round().max(32.0) as i32;
        let height = (rect.height() * scale).round().max(32.0) as i32;
        if self.attached_editor == Some(host_id) {
            if let Some(window) = self.editor_windows.get_mut(&host_id) {
                window.set_rect(x, y, width, height);
                window.set_visible(true);
            }
            return;
        }
        let Some(ui) = self.hosted.get_mut(&host_id) else {
            return;
        };
        let Some(window) =
            pulseforge_hosting::editor::EditorWindow::new(self.main_hwnd, x, y, width, height)
        else {
            self.hosted_errors.insert(
                host_id,
                "Could not create a native window for the plug-in editor.".to_owned(),
            );
            return;
        };
        match ui.open_editor(
            pulseforge_hosting::ParentWindow(window.handle()),
            width as u32,
            height as u32,
        ) {
            Ok((real_width, real_height)) => {
                // Plug-ins may ignore the requested size; remember the real one
                // so the wrapper reserves enough room next frame.
                if let Some(meta) = self.hosted_meta.get_mut(&host_id) {
                    meta.editor_size = (real_width, real_height);
                }
                window.set_rect(x, y, real_width as i32, real_height as i32);
                self.editor_windows.insert(host_id, window);
                self.attached_editor = Some(host_id);
            }
            Err(error) => {
                self.hosted_errors.insert(host_id, error.to_string());
            }
        }
    }

    /// Hosted plug-in window body: identity, parameter knobs and the editor area.
    fn render_hosted_plugin(&mut self, ui: &mut egui::Ui, host_id: u64) {
        let Some(meta) = self.hosted_meta.get(&host_id) else {
            let message = self
                .hosted_errors
                .get(&host_id)
                .cloned()
                .unwrap_or_else(|| "Plug-in is not loaded.".to_owned());
            ui.colored_label(egui::Color32::from_rgb(220, 120, 110), message);
            return;
        };
        // Only the header lives in egui; the plug-in draws its own parameters in
        // its own editor, so no parameter strip is shown here.
        let info = meta.info.clone();
        let preferred = meta.editor_size;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&info.name).strong());
            if !info.vendor.is_empty() {
                ui.label(
                    egui::RichText::new(&info.vendor)
                        .small()
                        .color(egui::Color32::from_gray(150)),
                );
            }
            if let Some((id, name)) = self.last_tweaked.clone()
                && id == host_id
            {
                if ui
                    .button("Automate")
                    .on_hover_text(format!("Create an automation clip for {name}"))
                    .clicked()
                {
                    self.create_plugin_automation(host_id, &name);
                }
                ui.label(
                    egui::RichText::new(name)
                        .small()
                        .color(egui::Color32::from_gray(160)),
                );
            }
        });
        if let Some(error) = self.hosted_errors.get(&host_id).cloned() {
            ui.colored_label(egui::Color32::from_rgb(230, 140, 110), error);
        }
        // The editor fills whatever space the window gives it, so resizing the
        // window resizes the plug-in.
        let scale = self.last_pixels_per_point.max(0.5);
        let min = egui::vec2(
            (preferred.0 as f32 / scale).max(320.0),
            (preferred.1 as f32 / scale).max(200.0),
        );
        let available = ui.available_size();
        let size = egui::vec2(available.x.max(min.x), available.y.max(min.y));
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 4.0, egui::Color32::from_gray(10));
        self.sync_editor_window(host_id, rect);
        self.resize_editor_to(host_id, rect);
    }

    /// External plug-in wrapper: shows the bundle identity plus an editor area.
    /// Fallback shown until an instrument's bundle has been loaded.
    fn render_external_plugin_wrapper(&mut self, ui: &mut egui::Ui, instrument_id: usize) {
        let host_id = pulseforge_core::hosted::instrument_host_id(instrument_id);
        if self.hosted.contains_key(&host_id) {
            self.render_hosted_plugin(ui, host_id);
            return;
        }
        let path = self
            .project
            .instruments
            .get(instrument_id)
            .map(|instrument| instrument.plugin_path.clone())
            .unwrap_or_default();
        let format = self
            .external_plugins
            .iter()
            .find(|plugin| plugin.path == path)
            .map_or("Plug-in", |plugin| plugin.format.label());
        let metadata = self.metadata_for(&path);
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_gray(18))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format).strong().color(self.ui_accent()));
                    ui.label(egui::RichText::new(&metadata.name).strong());
                });
                ui.label(egui::RichText::new(&path).small());
                if let Some(error) = self.hosted_errors.get(&host_id) {
                    ui.colored_label(egui::Color32::from_rgb(230, 140, 110), error);
                }
                if ui.button("Load plug-in").clicked() {
                    self.load_host(host_id, &path, true);
                }
            });
    }

    /// Colour of a channel row: its custom colour or the generated one.
    fn instrument_color(&self, instrument_id: usize) -> egui::Color32 {
        self.project
            .instruments
            .get(instrument_id)
            .and_then(|instrument| instrument.color)
            .map_or_else(
                || lane_color(instrument_id),
                |[r, g, b]| egui::Color32::from_rgb(r, g, b),
            )
    }

    /// Commits the inline instrument rename started from the channel menu.
    fn commit_instrument_rename(&mut self) {
        let Some(instrument_id) = self.renaming_instrument.take() else {
            return;
        };
        let name = self.rename_buffer.trim().to_owned();
        if name.is_empty() {
            return;
        }
        if let Some(instrument) = self.project.instruments.get_mut(instrument_id) {
            instrument.name = name;
        }
        self.sync_project();
    }

    /// Number of arrangement steps a sample spans at the current tempo.
    fn steps_for_sample(&self, sample: &audio::SampleData) -> u64 {
        let seconds = sample.frames.len() as f64 / f64::from(sample.sample_rate.max(1));
        let steps_per_second = f64::from(self.project.bpm) / 60.0 * 4.0;
        (seconds * steps_per_second).round().max(1.0) as u64
    }

    /// Same semantics for an instrument's power switch.
    fn toggle_instrument_switch(&mut self, instrument: usize) {
        let (shift, ctrl) = (self.mod_shift, self.mod_ctrl);
        let Some(item) = self.project.instruments.get_mut(instrument) else {
            return;
        };
        if shift {
            item.locked = !item.locked;
            self.status = if item.locked {
                format!("{} locked.", item.name)
            } else {
                format!("{} unlocked.", item.name)
            };
        } else if ctrl {
            item.solo = !item.solo;
            self.status = if item.solo {
                format!("{} soloed.", item.name)
            } else {
                format!("{} unsoloed.", item.name)
            };
        } else {
            // See `toggle_lane_switch`: a lock protects against global changes,
            // never against the user's own click.
            item.enabled = !item.enabled;
            self.status = if item.locked {
                format!("{} {} (locked).", item.name, power_word(item.enabled))
            } else {
                format!("{} {}.", item.name, power_word(item.enabled))
            };
        }
        self.push_history();
        self.sync_project();
    }

    /// (shift, ctrl) modifiers for selection gestures.
    fn selection_modifiers(&self) -> (bool, bool) {
        let (input_shift, input_ctrl) = (self.mod_shift, self.mod_ctrl);
        (input_shift, input_ctrl)
    }

    /// True when `item` is part of the current selection.
    fn is_selected(&self, item: Selection) -> bool {
        self.selection == Some(item) || self.extra_selection.contains(&item)
    }

    /// Replaces the selection, or extends it when shift/ctrl is held.
    fn select_item(&mut self, item: Selection, shift: bool, ctrl: bool) {
        if shift {
            // Shift always toggles membership of the clicked item.
            if self.is_selected(item) {
                self.extra_selection.retain(|existing| *existing != item);
                if self.selection == Some(item) {
                    // Promote another selected item so the selection never
                    // points at something that was just removed.
                    self.selection = self.extra_selection.first().copied();
                    if self.selection.is_some() {
                        self.extra_selection.remove(0);
                    }
                }
                if self.selection.is_none() {
                    self.selection_anchor = None;
                }
            } else if self.selection.is_none() {
                self.selection = Some(item);
                self.selection_anchor = Some(item);
            } else {
                self.extra_selection.push(item);
                self.selection_anchor.get_or_insert(item);
            }
            return;
        }
        if ctrl {
            // Ctrl+click extends to a range from the anchor. The range is built
            // from the concrete entries of the same kind, so clips and channels
            // no longer resolve to notes.
            if let Some(anchor) = self.selection_anchor
                && let Some(entries) = self.selection_between(anchor, item)
            {
                if self.selection.is_none() {
                    self.selection = Some(anchor);
                }
                self.extra_selection.clear();
                for entry in entries {
                    if self.selection != Some(entry) && !self.extra_selection.contains(&entry) {
                        self.extra_selection.push(entry);
                    }
                }
                return;
            }
            if self.selection.is_none() {
                self.selection = Some(item);
            } else if !self.is_selected(item) {
                self.extra_selection.push(item);
            }
            self.selection_anchor.get_or_insert(item);
            return;
        }
        self.selection = Some(item);
        self.extra_selection.clear();
        self.selection_anchor = Some(item);
    }

    /// Selects a piano note, extending the selection when requested.
    fn select_note(
        &mut self,
        _index: usize,
        instrument: usize,
        pitch: u8,
        start_step: u64,
        shift: bool,
        ctrl: bool,
    ) {
        let item = Selection::Note {
            pattern: self.selected_pattern,
            instrument,
            pitch,
            start_step,
        };
        self.selected_instrument = instrument;
        self.status = format!("Selected note {} at step {}.", note_name(pitch), start_step);
        self.select_item(item, shift, ctrl);
    }

    /// Every scale offered in the piano-roll toolbar.
    const SCALE_NAMES: [&'static str; 11] = [
        "Chromatic",
        "Major",
        "Minor",
        "Harmonic minor",
        "Dorian",
        "Phrygian",
        "Lydian",
        "Mixolydian",
        "Pentatonic major",
        "Pentatonic minor",
        "Blues",
    ];

    fn scale_contains(&self, pitch: u8) -> bool {
        scale_contains_pitch(self.settings.scale_root, &self.settings.scale_name, pitch)
    }

    /// Nearest pitch inside the scale; used when note snapping is on.
    fn snap_pitch_to_scale(&self, pitch: u8) -> u8 {
        if !self.settings.scale_snap {
            return pitch;
        }
        snap_pitch_into_scale(self.settings.scale_root, &self.settings.scale_name, pitch)
    }

    /// Moves every selected note (or the whole pattern when nothing is
    /// selected) by `semitones` and/or by `steps` snap units.
    fn shift_selected_notes(&mut self, semitones: i32, steps: i32, use_snap: bool) {
        let snap = if use_snap {
            self.project.snap_ticks.max(1) as i64
        } else {
            0
        };
        let step_delta = steps as i64 * snap;
        let selected: Vec<(u8, u64)> = self
            .extra_selection
            .iter()
            .chain(self.selection.iter())
            .filter_map(|item| match item {
                Selection::Note {
                    pitch, start_step, ..
                } => Some((*pitch, *start_step)),
                _ => None,
            })
            .collect();
        if semitones == 0 && step_delta == 0 {
            return;
        }
        self.push_history();
        let Some(pattern) = self.project.patterns.get_mut(self.selected_pattern) else {
            return;
        };
        let mut moved = 0;
        for note in &mut pattern.notes {
            let key = (note.pitch, note.start_step);
            if !selected.is_empty() && !selected.contains(&key) {
                continue;
            }
            let pitch = (i32::from(note.pitch) + semitones).clamp(0, 127) as u8;
            let step = if step_delta >= 0 {
                note.start_step.saturating_add(step_delta as u64)
            } else {
                note.start_step.saturating_sub((-step_delta) as u64)
            };
            note.pitch = pitch;
            note.start_step = step;
            moved += 1;
        }
        // Keep the selection pointing at the notes that just moved.
        let shift = |item: &mut Selection| {
            if let Selection::Note {
                pitch, start_step, ..
            } = item
            {
                *pitch = (i32::from(*pitch) + semitones).clamp(0, 127) as u8;
                *start_step = if step_delta >= 0 {
                    start_step.saturating_add(step_delta as u64)
                } else {
                    start_step.saturating_sub((-step_delta) as u64)
                };
            }
        };
        if let Some(item) = self.selection.as_mut() {
            shift(item);
        }
        for item in &mut self.extra_selection {
            shift(item);
        }
        if moved > 0 {
            self.status = format!("Moved {moved} note(s).");
            self.sync_project();
        }
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.extra_selection.clear();
        self.selection_anchor = None;
    }

    /// Ordered list of the notes in the current pattern, used for range select.
    fn note_order(&self) -> Vec<Selection> {
        self.project
            .patterns
            .get(self.selected_pattern)
            .map(|pattern| {
                let mut notes: Vec<_> = pattern.notes.iter().collect();
                notes.sort_by_key(|note| (note.start_step, note.pitch));
                notes
                    .into_iter()
                    .map(|note| Selection::Note {
                        pattern: self.selected_pattern,
                        instrument: note.instrument_id,
                        pitch: note.pitch,
                        start_step: note.start_step,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clip order across the arrangement, used for ctrl range selection.
    fn clip_order(&self) -> Vec<Selection> {
        let mut clips = Vec::new();
        for (lane, data) in self.project.lanes.iter().enumerate() {
            for (clip, item) in data.clips.iter().enumerate() {
                clips.push((item.start_step, lane, clip));
            }
        }
        clips.sort_unstable();
        clips
            .into_iter()
            .map(|(_, lane, clip)| Selection::Clip { lane, clip })
            .collect()
    }

    /// Every selection between two selections of the same kind, inclusive.
    ///
    /// Returning the entries rather than indices matters because the kinds have
    /// separate orderings (notes, clips, channels); an index taken from one list
    /// would address a different item in another.
    fn selection_between(&self, from: Selection, to: Selection) -> Option<Vec<Selection>> {
        let list = match (from, to) {
            (Selection::Note { .. }, Selection::Note { .. }) => self.note_order(),
            (Selection::Clip { .. }, Selection::Clip { .. }) => self.clip_order(),
            (Selection::MixerChannel(_), Selection::MixerChannel(_)) => self.channel_order(),
            (Selection::Instrument(_), Selection::Instrument(_)) => {
                (0..self.project.instruments.len())
                    .map(Selection::Instrument)
                    .collect()
            }
            _ => return None,
        };
        let start = list.iter().position(|item| *item == from)?;
        let end = list.iter().position(|item| *item == to)?;
        let (low, high) = (start.min(end), start.max(end));
        Some(list[low..=high].to_vec())
    }

    /// Mixer channels, master last, used for range selection in the mixer and
    /// the arrangement track headers.
    fn channel_order(&self) -> Vec<Selection> {
        (0..=self.project.lanes.len())
            .map(Selection::MixerChannel)
            .collect()
    }

    /// Selects an arrangement lane (and its mixer channel) honouring the
    /// current modifier keys.
    fn select_lane(&mut self, lane: usize) {
        self.select_channel(lane);
    }

    /// Selects a mixer channel, honouring shift (toggle) and ctrl (range).
    fn select_channel(&mut self, channel: usize) {
        let (shift, ctrl) = (self.mod_shift, self.mod_ctrl);
        if channel < self.project.lanes.len() {
            self.selected_lane = channel;
        }
        self.selected_mixer_channel = channel;
        self.select_item(Selection::MixerChannel(channel), shift, ctrl);
    }

    /// Moves the global snap one entry up or down the fixed step list.
    fn step_snap(&mut self, direction: i32) {
        let current = self
            .project
            .snap_ticks
            .clamp(SNAP_STEPS[0], *SNAP_STEPS.last().unwrap_or(&128));
        let index = SNAP_STEPS
            .iter()
            .position(|value| *value >= current)
            .unwrap_or(0);
        let next = if direction > 0 {
            (index + 1).min(SNAP_STEPS.len() - 1)
        } else {
            index.saturating_sub(1)
        };
        self.project.snap_ticks = SNAP_STEPS[next];
        self.status = format!("Snap: {}", snap_label(SNAP_STEPS[next]));
        self.sync_project();
    }

    /// Accent colour from the theming page.
    fn ui_accent(&self) -> egui::Color32 {
        egui::Color32::from_rgb(
            self.settings.accent[0],
            self.settings.accent[1],
            self.settings.accent[2],
        )
    }

    /// Applies the current repeat settings to the selected note, or to every
    /// note at the selection's pitch when a note is selected.
    fn apply_repeat_to_selection(&mut self) {
        let Some(Selection::Note {
            pattern,
            instrument,
            pitch,
            start_step,
        }) = self.selection
        else {
            self.status = "Select a note in the piano roll first.".to_owned();
            return;
        };
        let count = self.note_repeat_count;
        let ticks = self.note_repeat_ticks;
        let decay = self.note_repeat_decay;
        let Some(pattern) = self.project.patterns.get_mut(pattern) else {
            return;
        };
        let mut updated = 0;
        for note in &mut pattern.notes {
            if note.instrument_id == instrument
                && note.pitch == pitch
                && note.start_step == start_step
            {
                note.repeat_count = count;
                note.repeat_ticks = ticks;
                note.repeat_decay = decay;
                updated += 1;
            }
        }
        if updated == 0 {
            self.status = "That note no longer exists.".to_owned();
            return;
        }
        self.sync_project();
        self.status = format!("{updated} note(s) updated with {count} repeat(s).");
    }

    /// Opens the rename prompt for a track. The mixer channel shares the name,
    /// so one action renames both.
    fn begin_rename_track(&mut self) {
        let lane = self.lane_for_selection();
        self.rename_buffer = self
            .project
            .lanes
            .get(lane)
            .map(|lane| lane.name.clone())
            .unwrap_or_default();
        self.renaming_track = Some(lane);
    }

    /// Track that rename/reorder actions target: the selected clip's lane, else
    /// the selected mixer channel.
    fn lane_for_selection(&self) -> usize {
        match self.selection {
            Some(Selection::Clip { lane, .. }) => lane.min(self.project.lanes.len() - 1),
            _ => self.selected_lane.min(self.project.lanes.len() - 1),
        }
    }

    fn commit_rename_track(&mut self) {
        let Some(lane) = self.renaming_track.take() else {
            return;
        };
        let name = self.rename_buffer.trim().to_owned();
        if name.is_empty() {
            self.status = "Track name cannot be empty.".to_owned();
            return;
        }
        if let Some(target) = self.project.lanes.get_mut(lane) {
            target.name = name.clone();
        }
        self.push_history();
        self.status = format!("Renamed track to {name} (mixer channel renamed too).");
        self.sync_project();
    }

    /// Selects an editing tool, remembering that the arrangement has no Join.
    fn set_tool(&mut self, tool: PianoTool) {
        self.piano_tool = tool;
        // The hotkeys are shared: pressing S must select in the arrangement too,
        // not just in the piano roll. Join exists only in the piano roll, so the
        // arrangement keeps whatever tool it had.
        if tool != PianoTool::Join {
            self.arrangement_tool = tool;
        }
        let name = match tool {
            PianoTool::Pen => "Pen",
            PianoTool::Select => "Select",
            PianoTool::Brush => "Brush",
            PianoTool::Join => "Join (piano roll only)",
            PianoTool::Cut => "Cut",
        };
        self.status = format!("{name} tool.");
    }

    fn sender(&self) -> Option<SyncSender<AudioCommand>> {
        self.audio.as_ref().map(|audio| audio.sender.clone())
    }

    fn send_audio(&mut self, command: AudioCommand) -> bool {
        let Some(sender) = self.sender() else {
            self.status = self
                .audio_error
                .clone()
                .unwrap_or_else(|| "Audio output is unavailable.".to_owned());
            return false;
        };
        match sender.try_send(command) {
            Ok(()) => true,
            Err(error) => {
                self.status = format!("Audio update failed: {error}");
                false
            }
        }
    }

    fn sync_project(&mut self) {
        self.project.normalize();
        let routes = match self.midi_routes.lock() {
            Ok(mut routes) => {
                *routes = self.project.midi_routes.to_vec();
                true
            }
            Err(_) => false,
        };
        if !routes {
            self.status = "MIDI routing table could not be updated.".to_owned();
        }
        self.project_dirty = true;
        self.flush_project();
    }

    /// Coalesces project snapshots so continuous edits (knob drags, painting)
    /// cannot flood the audio thread with full project rebuilds.
    fn flush_project(&mut self) {
        const MIN_SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);
        if !self.project_dirty || self.last_project_sync.elapsed() < MIN_SYNC_INTERVAL {
            return;
        }
        self.project_dirty = false;
        self.last_project_sync = std::time::Instant::now();
        if let Some(audio) = &self.audio {
            if let Err(error) = audio.set_project(self.project.clone()) {
                self.status = format!("Could not queue project update: {error}");
            }
        } else {
            self.send_audio(AudioCommand::SetProject(Box::new(self.project.clone())));
        }
    }

    /// Snapshots the project so the next edit can be undone. Callers use this
    /// at the start of a gesture or discrete action, never per frame.
    fn push_history(&mut self) {
        const HISTORY_LIMIT: usize = 64;
        self.history.push(self.project.clone());
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
        self.redo_history.clear();
    }

    fn undo(&mut self) {
        let Some(previous) = self.history.pop() else {
            self.status = "Nothing to undo.".to_owned();
            return;
        };
        self.redo_history
            .push(std::mem::replace(&mut self.project, previous));
        self.project.normalize();
        self.sync_project();
        self.status = "Undo.".to_owned();
    }

    fn redo(&mut self) {
        let Some(next) = self.redo_history.pop() else {
            self.status = "Nothing to redo.".to_owned();
            return;
        };
        self.history
            .push(std::mem::replace(&mut self.project, next));
        self.project.normalize();
        self.sync_project();
        self.status = "Redo.".to_owned();
    }

    /// Advances the displayed playhead using the audio thread's published
    /// position plus a short extrapolation, so motion is frame-rate smooth
    /// instead of stepping once per audio buffer.
    fn advance_playhead(&mut self) {
        let Some(audio) = &self.audio else {
            self.playhead_display = 0.0;
            return;
        };
        let raw = audio
            .playhead_position
            .load(std::sync::atomic::Ordering::Relaxed);
        if raw != self.playhead_raw {
            self.playhead_raw = raw;
            self.playhead_anchor = std::time::Instant::now();
        }
        let base = f64::from_bits(raw);
        if !self.playing {
            self.playhead_display = base;
            return;
        }
        let steps_per_second = f64::from(self.project.bpm) / 60.0 * 4.0;
        let elapsed = self.playhead_anchor.elapsed().as_secs_f64().min(0.12);
        let predicted = base + elapsed * steps_per_second;
        let delta = predicted - self.playhead_display;
        if delta.abs() > 2.0 || delta < -0.05 {
            // Transport jump (loop wrap or seek): follow it immediately.
            self.playhead_display = predicted;
        } else {
            self.playhead_display += delta * 0.4;
        }
    }

    /// Playhead position in steps, wrapped when previewing a single pattern.
    fn playhead_step(&self) -> f64 {
        let position = self.playhead_display.max(0.0);
        if self.selected_pattern_playback {
            let length = self
                .project
                .patterns
                .get(self.selected_pattern)
                .map_or(1, |pattern| pattern.length_steps.max(1));
            position % length as f64
        } else if self.playing
            && self.loop_enabled
            && let Some((start, end)) = self.arrangement_loop
            && end > start
            && position >= end as f64
        {
            let span = (end - start) as f64;
            start as f64 + (position - end as f64) % span
        } else {
            position
        }
    }

    /// Places the active arrangement source on a lane, returning the new clip
    /// index. Shared by the pen, brush and drag-and-drop paths.
    fn place_source_clip(
        &mut self,
        lane: usize,
        start_step: u64,
        source: ClipSource,
    ) -> Option<usize> {
        if lane >= self.project.lanes.len() {
            return None;
        }
        // Audio clips take their length from the sample so the clip matches what
        // is heard; the sample is read here once, before the project snapshot is
        // sent, because the engine rebuilds clip runtimes on project updates.
        let mut loaded_sample = None;
        let (length_steps, source) = match source {
            ClipSource::Pattern { pattern_id } => {
                let length = self
                    .project
                    .patterns
                    .get(pattern_id)
                    .map_or(16, |pattern| pattern.length_steps);
                (length, ClipSource::Pattern { pattern_id })
            }
            ClipSource::Audio { path } => match audio::read_wav(&path) {
                Ok(sample) => {
                    let steps = self.steps_for_sample(&sample);
                    loaded_sample = Some(sample);
                    (steps, ClipSource::Audio { path })
                }
                Err(error) => {
                    self.status = format!("WAV could not load: {error}");
                    (
                        self.new_audio_length_steps.max(1),
                        ClipSource::Audio { path },
                    )
                }
            },
        };
        let clip_index = self.add_arrangement_clip(lane, start_step, length_steps, source.clone());
        self.selected_lane = lane;
        self.selection = Some(Selection::Clip {
            lane,
            clip: clip_index,
        });
        self.sync_project();
        if let ClipSource::Audio { path } = &source {
            match loaded_sample {
                Some(sample) => {
                    self.send_audio(AudioCommand::LoadAudioClip {
                        path: path.clone(),
                        sample,
                    });
                    self.status = format!(
                        "Placed {} ({} steps) on this mixer lane.",
                        clip_label(path),
                        length_steps
                    );
                }
                None => {
                    self.status = format!("Placed {path} but the WAV could not be read.");
                }
            }
        } else {
            self.status =
                "Placed pattern clip; its instruments route to this mixer lane.".to_owned();
        }
        Some(clip_index)
    }

    /// Splits the clip under the pointer at `step`, mirroring the FL Studio
    /// slice tool. Returns whether anything was cut.
    fn slice_clip_at(&mut self, lane: usize, step: u64, snap_ticks: u64) -> bool {
        let Some(lane_data) = self.project.lanes.get_mut(lane) else {
            return false;
        };
        let Some(index) = lane_data.clips.iter().position(|clip| {
            clip.start_step < step && step < clip.start_step.saturating_add(clip.length_steps)
        }) else {
            return false;
        };
        let clip = lane_data.clips[index].clone();
        let cut = snap_arrangement_step(step, snap_ticks).max(clip.start_step + 1);
        let end = clip.start_step.saturating_add(clip.length_steps);
        if cut >= end {
            return false;
        }
        lane_data.clips[index].length_steps = cut - clip.start_step;
        lane_data.clips.push(ArrangementClip {
            start_step: cut,
            length_steps: end - cut,
            offset_steps: clip
                .offset_steps
                .saturating_add(cut.saturating_sub(clip.start_step)),
            source: clip.source,
            ..clip
        });
        lane_data.clips.sort_by_key(|clip| clip.start_step);
        self.status = "Sliced clip.".to_owned();
        true
    }

    /// Routes an instrument to the next free mixer insert and renames that
    /// insert after the instrument, mirroring Ctrl+L in the channel rack.
    fn route_instrument_to_next_insert(&mut self, instrument_id: usize) {
        let Some(instrument) = self.project.instruments.get(instrument_id) else {
            return;
        };
        let name = instrument.name.clone();
        let used: Vec<usize> = self
            .project
            .instruments
            .iter()
            .enumerate()
            .filter(|(id, _)| *id != instrument_id)
            .map(|(_, other)| other.default_mixer_lane)
            .collect();
        let Some(lane) = (0..self.project.lanes.len()).find(|lane| !used.contains(lane)) else {
            self.status = "No free mixer insert is available.".to_owned();
            return;
        };
        self.push_history();
        self.project.instruments[instrument_id].default_mixer_lane = lane;
        self.project.lanes[lane].name = name;
        self.selected_mixer_channel = lane;
        self.sync_project();
        self.status = format!("Routed to mixer insert {}.", lane + 1);
    }

    fn load_instrument_sample(&mut self, instrument_id: usize) {
        let Some(instrument) = self.project.instruments.get(instrument_id) else {
            self.status = "That instrument no longer exists.".to_owned();
            return;
        };
        let path = instrument.sample_path.clone();
        if path.trim().is_empty() {
            self.status = "Enter a WAV path before loading the instrument sample.".to_owned();
            return;
        }
        // Decoding happens on a worker thread; the audio side is updated when
        // the task lands.
        self.status = format!("Loading {path}…");
        self.tasks
            .decode_sample(path, tasks::SampleUse::Instrument(instrument_id));
    }

    fn add_audio_clip(&mut self, start_step: u64, lane: usize) {
        let path = self.new_audio_path.trim().to_owned();
        if path.is_empty() {
            self.status = "Enter a WAV path for the audio clip.".to_owned();
            return;
        }
        if lane >= self.project.lanes.len() {
            self.status = "The target arrangement lane is unavailable.".to_owned();
            return;
        }
        let clip_index = self.add_arrangement_clip(
            lane,
            start_step,
            self.new_audio_length_steps.max(1),
            ClipSource::Audio { path: path.clone() },
        );
        self.selected_lane = lane;
        self.selection = Some(Selection::Clip {
            lane,
            clip: clip_index,
        });
        self.sync_project();
        self.status = format!("Added audio clip from {path}; decoding…");
        self.tasks.decode_sample(path, tasks::SampleUse::Clip);
    }

    fn add_arrangement_clip(
        &mut self,
        lane: usize,
        start_step: u64,
        length_steps: u64,
        source: ClipSource,
    ) -> usize {
        let insertion_index = self.project.lanes[lane]
            .clips
            .iter()
            .filter(|clip| clip.start_step <= start_step)
            .count();
        self.project.lanes[lane].clips.insert(
            insertion_index,
            ArrangementClip {
                start_step,
                length_steps: length_steps.max(1),
                offset_steps: 0,
                source,
                ..ArrangementClip::default()
            },
        );
        insertion_index
    }

    fn load_audio_clip_samples(&mut self) {
        if self.sender().is_none() {
            self.status = self
                .audio_error
                .clone()
                .unwrap_or_else(|| "Audio output is unavailable.".to_owned());
            return;
        }
        // Each clip decodes on its own worker thread; a project with many
        // audio clips therefore loads in parallel instead of one WAV at a time.
        let mut paths = BTreeSet::new();
        for lane in self.project.lanes.iter() {
            for clip in lane.clips.iter() {
                if let ClipSource::Audio { path } = &clip.source {
                    paths.insert(path.clone());
                }
            }
        }
        for path in paths {
            self.tasks.decode_sample(path, tasks::SampleUse::Clip);
        }
    }

    /// Asks every hosted plug-in for its opaque state and stores it in the
    /// project, so a reload restores exactly what the user had.
    fn capture_plugin_states(&mut self) {
        for instrument_id in 0..self.project.instruments.len() {
            if self.project.instruments[instrument_id]
                .plugin_path
                .is_empty()
            {
                continue;
            }
            let id = pulseforge_core::hosted::instrument_host_id(instrument_id);
            if let Some(state) = self
                .hosted
                .get_mut(&id)
                .and_then(pulseforge_hosting::HostedUi::save_state)
            {
                self.project.instruments[instrument_id].plugin_state = state;
            }
        }
        let lane_count = self.project.lanes.len();
        for channel in 0..=lane_count {
            let slot_count = if channel == lane_count {
                self.project.master.inserts.len()
            } else {
                self.project.lanes[channel].inserts.len()
            };
            for slot in 0..slot_count {
                let has_plugin = self
                    .effect_slot_name(channel, slot)
                    .is_some_and(|_| self.effect_slot_path(channel, slot).is_some());
                if !has_plugin {
                    continue;
                }
                let id = pulseforge_core::hosted::effect_host_id(channel, slot);
                if let Some(state) = self
                    .hosted
                    .get_mut(&id)
                    .and_then(pulseforge_hosting::HostedUi::save_state)
                    && let Some(effect) = self.effect_slot_mut(channel, slot)
                {
                    effect.plugin_state = state;
                }
            }
        }
    }

    /// Plug-in path of an insert slot, when it holds a hosted bundle.
    fn effect_slot_path(&self, channel: usize, slot: usize) -> Option<String> {
        let slots = if channel == self.project.lanes.len() {
            &self.project.master.inserts
        } else {
            &self.project.lanes.get(channel)?.inserts
        };
        let path = &slots.get(slot)?.plugin_path;
        (!path.is_empty()).then(|| path.clone())
    }

    fn save_project(&mut self) {
        self.capture_plugin_states();
        self.project.normalize();
        // Serialization and the write are handed to a worker thread; the clone
        // is cheap compared with encoding a large project.
        let path = self.project_path.clone();
        self.status = format!("Saving {path}…");
        self.tasks.save_project(path, self.project.clone());
    }

    fn load_project(&mut self) {
        let path = self.project_path.clone();
        self.status = format!("Loading {path}…");
        self.tasks.load_project(path);
    }

    /// Applies a project that finished loading: resets the selection, pushes it
    /// to the engine, and restores everything the project references.
    fn adopt_project(&mut self, mut project: Project, path: &str) {
        project.normalize();
        self.project = project;
        self.selected_pattern = 0;
        self.selected_instrument = 0;
        self.selected_lane = 0;
        self.selected_mixer_channel = 0;
        self.selection = None;
        self.sync_project();
        self.load_audio_clip_samples();
        self.load_instrument_samples();
        self.restore_hosted_plugins();
        self.status = format!("Loaded project from {path}.");
    }

    /// Drains finished background jobs; call once per frame.
    fn poll_tasks(&mut self) {
        while let Some(task) = self.tasks.try_recv() {
            self.apply_task(task);
        }
    }

    /// Applies one finished job to the app state.
    fn apply_task(&mut self, task: tasks::Task) {
        use tasks::{SampleUse, Task};
        match task {
            Task::PluginsScanned { plugins, error } => {
                self.status = format!("Found {} external plug-in(s).", plugins.len());
                self.external_plugins = plugins;
                self.plugin_scan_error = error;
            }
            Task::SamplesScanned { files } => {
                self.status = format!("{} sample(s) found.", files.len());
                self.sample_files = files;
            }
            Task::SampleDecoded {
                path,
                purpose,
                result,
            } => match (result, purpose) {
                (Ok(sample), SampleUse::Instrument(instrument_id)) => {
                    let frames = sample.frames.len();
                    if self.send_audio(AudioCommand::LoadInstrumentSample {
                        instrument_id,
                        sample,
                    }) {
                        self.status = format!("Loaded {frames} frames into the instrument.");
                    }
                }
                (Ok(sample), SampleUse::Clip) => {
                    let frames = sample.frames.len();
                    if self.send_audio(AudioCommand::LoadAudioClip {
                        path: path.clone(),
                        sample,
                    }) {
                        self.status = format!("Loaded {frames} frames for {path}.");
                    }
                }
                (Ok(sample), SampleUse::Preview) => {
                    self.send_audio(AudioCommand::PreviewSample(Box::new(sample)));
                    self.status = format!("Previewing {path}");
                }
                (Err(error), SampleUse::Instrument(_)) => {
                    self.status = format!("Sample load failed: {error}");
                }
                (Err(error), _) => self.status = format!("Could not read {path}: {error}"),
            },
            Task::ProjectLoaded { path, result } => match result {
                Ok(project) => self.adopt_project(*project, &path),
                Err(error) => self.status = error,
            },
            Task::ProjectSaved { path, result } => match result {
                Ok(()) => self.status = format!("Saved project to {path}."),
                Err(error) => self.status = error,
            },
        }
    }

    /// Re-instantiates every VST3/CLAP bundle the project references. Hosted
    /// instances are never serialized themselves, so a project load has to
    /// rebuild them or the channels would come back silent.
    fn restore_hosted_plugins(&mut self) {
        for id in self.hosted.keys().copied().collect::<Vec<_>>() {
            self.unload_host(id);
        }
        let mut jobs = Vec::new();
        for instrument_id in 0..self.project.instruments.len() {
            let instrument = &self.project.instruments[instrument_id];
            if instrument.synth_kind == SynthKind::External && !instrument.plugin_path.is_empty() {
                jobs.push((
                    pulseforge_core::hosted::instrument_host_id(instrument_id),
                    instrument.plugin_path.clone(),
                    true,
                ));
            }
        }
        let lane_count = self.project.lanes.len();
        for channel in 0..=lane_count {
            let slots = if channel == lane_count {
                &self.project.master.inserts
            } else {
                &self.project.lanes[channel].inserts
            };
            for (slot_index, slot) in slots.iter().enumerate() {
                if !slot.enabled || slot.plugin_path.is_empty() {
                    continue;
                }
                jobs.push((
                    pulseforge_core::hosted::effect_host_id(channel, slot_index),
                    slot.plugin_path.clone(),
                    false,
                ));
            }
        }
        for (id, path, instrument) in jobs {
            self.load_host(id, &path, instrument);
        }
    }

    fn load_instrument_samples(&mut self) {
        for instrument_id in 0..self.project.instruments.len() {
            if !self.project.instruments[instrument_id]
                .sample_path
                .is_empty()
            {
                self.load_instrument_sample(instrument_id);
            }
        }
    }

    fn query_midi_ports() -> Result<Vec<String>, String> {
        let input = MidiInput::new("PulseForge MIDI Discovery")
            .map_err(|error| format!("Could not enumerate MIDI inputs: {error}"))?;
        input
            .ports()
            .iter()
            .enumerate()
            .map(|(index, port)| {
                input.port_name(port).map_err(|error| {
                    format!("Could not read MIDI input {} name: {error}", index + 1)
                })
            })
            .collect()
    }

    fn connect_midi(&mut self, index: usize) {
        self.midi_connection.take();
        let Some(sender) = self.sender() else {
            self.status = self
                .audio_error
                .clone()
                .unwrap_or_else(|| "Audio output is unavailable.".to_owned());
            return;
        };
        let mut input = match MidiInput::new("PulseForge MIDI Input") {
            Ok(input) => input,
            Err(error) => {
                self.status = format!("Could not initialize MIDI: {error}");
                return;
            }
        };
        input.ignore(Ignore::None);
        let ports = input.ports();
        let Some(port) = ports.get(index) else {
            self.status = "The selected MIDI input is no longer available.".to_owned();
            return;
        };
        let routes = Arc::clone(&self.midi_routes);
        let callback_error = Arc::clone(&self.midi_callback_error);
        match input.connect(
            port,
            "PulseForge MIDI Input",
            move |_timestamp, message, _| {
                if message.len() < 3 {
                    return;
                }
                let status = message[0];
                let kind = status & 0xF0;
                if kind != 0x80 && kind != 0x90 {
                    return;
                }
                let channel = usize::from(status & 0x0F);
                let instrument_id = match routes.lock() {
                    Ok(routes) => routes.get(channel).copied().unwrap_or(0),
                    Err(_) => {
                        if let Ok(mut error) = callback_error.lock() {
                            *error = Some("MIDI route table became unavailable.".to_owned());
                        }
                        return;
                    }
                };
                let event = AudioCommand::MidiNote {
                    instrument_id,
                    note: message[1],
                    velocity: message[2],
                    on: kind == 0x90 && message[2] > 0,
                };
                if let Err(error) = sender.try_send(event)
                    && let Ok(mut callback_error) = callback_error.lock()
                {
                    *callback_error = Some(format!("MIDI event queue failed: {error}"));
                }
            },
            (),
        ) {
            Ok(connection) => {
                self.midi_connection = Some(connection);
                self.selected_midi_port = Some(index);
                self.status = format!(
                    "Connected MIDI input: {}",
                    self.midi_ports.get(index).map_or("Device", String::as_str)
                );
            }
            Err(error) => self.status = format!("Could not connect MIDI input: {error}"),
        }
    }






    fn effect_slot_name(&self, channel: usize, slot: usize) -> Option<String> {
        let slots = if channel == self.project.lanes.len() {
            &self.project.master.inserts
        } else {
            &self.project.lanes.get(channel)?.inserts
        };
        let name = &slots.get(slot)?.name;
        (!name.is_empty()).then(|| name.clone())
    }

    fn effect_slot_mut(&mut self, channel: usize, slot: usize) -> Option<&mut InsertFxSlot> {
        let slots = if channel == self.project.lanes.len() {
            &mut self.project.master.inserts
        } else {
            &mut self.project.lanes.get_mut(channel)?.inserts
        };
        slots.get_mut(slot)
    }


    /// Samples a parsed formula into a curve lane on the current automation
    /// target, so a formula behaves like any hand-drawn lane afterwards.
    fn add_formula_automation(&mut self, expression: &pulseforge_core::expr::Expression) {
        const LENGTH_STEPS: u64 = 64;
        const POINT_COUNT: u64 = 32;
        let target = if self.automation_effect_mode {
            ParameterTarget::InsertEffect {
                channel_id: self.automation_channel_id,
                slot_id: self.automation_slot_id,
                parameter: self.automation_effect_parameter,
            }
        } else {
            ParameterTarget::Instrument {
                instrument_id: self.automation_instrument_id,
                parameter: self.automation_instrument_parameter,
            }
        };
        let points: Vec<AutomationPoint> = (0..=POINT_COUNT)
            .map(|index| AutomationPoint {
                step: LENGTH_STEPS * index / POINT_COUNT,
                value: expression
                    .evaluate(index as f64 / POINT_COUNT as f64)
                    .clamp(0.0, 1.0) as f32,
                curve: AutomationCurve::Linear,
            })
            .collect();
        let label = parameter_target_label(&self.project, target);
        self.push_history();
        self.project.automation_tracks.push(AutomationTrack {
            name: format!("{label} (formula)"),
            target,
            start_step: 0,
            length_steps: LENGTH_STEPS,
            points,
            modulation_points: Vec::new(),
            enabled: true,
            source: project::AutomationSource::Curve,
        });
        self.sync_project();
        self.status = "Formula sampled into a new automation lane.".to_owned();
    }


}

impl PulseForgeApp {
    /// Current automated values for every automated parameter, used to show
    /// automation movement on knobs while transport is running.
    fn live_automated_values(&self) -> Vec<(ParameterTarget, f32)> {
        let Some(audio) = &self.audio else {
            return Vec::new();
        };
        // Also evaluated while stopped, so an automated control sits where the
        // playhead says it should instead of showing the stored value.
        let position = audio
            .playhead_steps
            .load(std::sync::atomic::Ordering::Relaxed) as f64;
        let bpm = self.project.bpm;
        let mut targets: Vec<ParameterTarget> = Vec::new();
        for track in &self.project.automation_tracks {
            if track.enabled
                && !targets.contains(&track.target)
                && position >= track.start_step as f64
                && position < track.start_step.saturating_add(track.length_steps) as f64
            {
                targets.push(track.target);
            }
        }
        let mut values = Vec::with_capacity(targets.len());
        for target in targets {
            if let Some(value) = self.automated_display_value(target, position, bpm) {
                values.push((target, value));
            }
        }
        values
    }

    fn automated_display_value(
        &self,
        target: ParameterTarget,
        position: f64,
        bpm: f32,
    ) -> Option<f32> {
        let base = self.project.parameter_value(target)?;
        let base_normalized = target.normalize_value(base);
        let mut normalized: Option<f32> = None;
        let mut relative = 0.0;
        let mut touched = false;
        for track in &self.project.automation_tracks {
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
                    relative += value;
                    continue;
                }
                normalized = Some(match normalized {
                    Some(total) => total + value - track.points[0].value,
                    None => value,
                });
                continue;
            }
            let multiplier = if track.modulation_points.is_empty() {
                1.0
            } else {
                Project::evaluate_curve(&track.modulation_points, position).clamp(0.0, 1.0)
            };
            let source_value = automation_source_preview(
                &track.source,
                target,
                &self.project.lanes,
                &self.project.patterns,
                bpm,
                track.start_step,
                position,
            );
            let contribution = base + (source_value - base) * multiplier;
            touched = true;
            normalized =
                Some(normalized.unwrap_or(base_normalized) + contribution - base_normalized);
        }
        if !touched {
            return None;
        }
        Some(
            target
                .denormalize_value((normalized.unwrap_or(base_normalized) + relative).clamp(0.0, 1.0)),
        )
    }
}

impl eframe::App for PulseForgeApp {
    fn update(&mut self, context: &egui::Context, frame: &mut eframe::Frame) {
        // Tag this thread and wrap the frame so the flame graph shows the UI
        // channel; nesting rank below this span is the render cost per widget
        // group that is instrumented explicitly.
        set_thread_channel(Channel::Ui);
        profile_scope!("ui frame");
        // Background results arrive here, before anything reads the state.
        self.poll_tasks();
        // Plug-in editors are native child windows, so the DAW window's handle
        // and scale factor are needed to place them under their egui wrapper.
        self.last_pixels_per_point = context.pixels_per_point();
        if self.main_hwnd == 0
            && let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(frame)
            && let raw_window_handle::RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            self.main_hwnd = win32.hwnd.get();
        }
        if let Some(audio) = &self.audio {
            audio.collect_retired_projects();
        }
        // Sampled once per frame; every enable/selection gesture reads these.
        let (shift, ctrl) = context.input(|input| {
            (
                input.modifiers.shift,
                input.modifiers.command || input.modifiers.ctrl,
            )
        });
        self.mod_shift = shift;
        self.mod_ctrl = ctrl;
        // Re-applying visuals every frame would be wasteful, so the theme is
        // pushed only when the settings differ from what is already active.
        let theme_key = egui::Id::new("applied_theme_stamp");
        let stamp = format!("{:?}", self.settings.theme);
        let applied: Option<String> = context.data(|data| data.get_temp(theme_key));
        if applied.as_deref() != Some(stamp.as_str()) {
            apply_theme(context, &self.settings.theme);
            context.data_mut(|data| data.insert_temp(theme_key, stamp));
        }
        // DPI-like interface scale, pushed only when it changes so the layout
        // is not invalidated every frame.
        let scale_key = egui::Id::new("applied_ui_scale");
        let applied_scale: Option<f32> = context.data(|data| data.get_temp(scale_key));
        if applied_scale != Some(self.settings.ui_scale) {
            context.set_zoom_factor(self.settings.ui_scale);
            context.data_mut(|data| data.insert_temp(scale_key, self.settings.ui_scale));
        }
        self.advance_playhead();
        self.poll_last_tweaked();
        self.flush_project();
        let dt = context
            .input(|input| input.stable_dt)
            .clamp(1.0 / 240.0, 0.1);
        self.last_frame_seconds = dt;
        let animations = self.settings.animations;
        let speed = self.settings.animation_speed;
        let lift_target = if matches!(self.selection, Some(Selection::Clip { .. })) {
            1.0
        } else {
            0.0
        };
        self.clip_lift.set(lift_target);
        self.drag_ghost.set(if self.pattern_drag.is_some() {
            1.0
        } else {
            0.0
        });
        self.inspector_fade
            .set(if self.windows.mixer { 1.0 } else { 0.0 });
        self.clip_lift.step(dt, animations, speed);
        self.drag_ghost.step(dt, animations, speed);
        self.inspector_fade.step(dt, animations, speed);
        if self.clip_lift.is_animating()
            || self.drag_ghost.is_animating()
            || self.inspector_fade.is_animating()
        {
            context.request_repaint();
        }
        let automated_targets: Vec<ParameterTarget> = self
            .project
            .automation_tracks
            .iter()
            .filter(|track| track.enabled)
            .map(|track| track.target)
            .collect();
        let automated_values = self.live_automated_values();
        context.data_mut(|data| {
            data.insert_temp(egui::Id::new(AUTOMATED_TARGETS_KEY), automated_targets);
            data.insert_temp(egui::Id::new(AUTOMATED_VALUES_KEY), automated_values);
        });
        let preview_pattern = self
            .selected_pattern_playback
            .then_some(self.selected_pattern);
        if preview_pattern != self.audio_preview_pattern {
            self.audio_preview_pattern = preview_pattern;
            self.send_audio(AudioCommand::SetSelectedPattern(preview_pattern));
        }
        let loop_range = self.loop_enabled.then_some(self.arrangement_loop).flatten();
        if loop_range != self.audio_loop_range {
            self.audio_loop_range = loop_range;
            self.send_audio(AudioCommand::SetLoop(loop_range));
        }
        let automation_request = context.data_mut(|data| {
            data.remove_temp::<ParameterTarget>(egui::Id::new("knob_add_automation"))
        });
        if let Some(target) = automation_request {
            match target {
                ParameterTarget::Instrument {
                    instrument_id,
                    parameter,
                } => {
                    self.automation_effect_mode = false;
                    self.automation_instrument_id = instrument_id;
                    self.automation_instrument_parameter = parameter;
                }
                ParameterTarget::InsertEffect {
                    channel_id,
                    slot_id,
                    parameter,
                } => {
                    self.automation_effect_mode = true;
                    self.automation_channel_id = channel_id;
                    self.automation_slot_id = slot_id;
                    self.automation_effect_parameter = parameter;
                }
                // Global targets are added straight to the automation list.
                ParameterTarget::PluginParameter { .. }
                | ParameterTarget::MixerChannel { .. }
                | ParameterTarget::AutomationLane { .. }
                | ParameterTarget::Tempo => {
                    self.create_automation_for_target(target);
                }
            }
            if let Some(value) = self.project.parameter_value(target) {
                let base = target.normalize_value(value);
                let name = parameter_target_label(&self.project, target);
                self.project.automation_tracks.push(AutomationTrack {
                    name,
                    target,
                    start_step: 0,
                    length_steps: 64,
                    points: vec![
                        AutomationPoint {
                            step: 0,
                            value: base,
                            curve: AutomationCurve::Linear,
                        },
                        AutomationPoint {
                            step: 16,
                            value: base,
                            curve: AutomationCurve::Linear,
                        },
                    ],
                    modulation_points: Vec::new(),
                    enabled: true,
                    source: project::AutomationSource::Curve,
                });
                self.sync_project();
            }
            self.windows.automation = true;
        }
        if !context.wants_keyboard_input() {
            let actions = self.pressed_actions(context);
            for action in actions {
                self.perform(action);
            }
        }
        egui::TopBottomPanel::top("toolbar")
            .exact_height(60.0)
            .show(context, |ui| self.render_toolbar(ui));
        egui::TopBottomPanel::bottom("footer").show(context, |ui| self.render_footer(ui));

        egui::SidePanel::left("clip_picker")
            .default_width(245.0)
            .min_width(190.0)
            .max_width(360.0)
            .resizable(true)
            .show(context, |ui| self.render_clip_picker(ui));

        egui::CentralPanel::default().show(context, |ui| {
            self.render_arrangement(ui, context);
        });

        let mut rack_open = self.windows.channel_rack;
        egui::Window::new("Channel Rack · Step Sequencer")
            .open(&mut rack_open)
            .default_size([1050.0, 290.0])
            .min_size([520.0, 220.0])
            .resizable(true)
            .show(context, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.render_channel_rack(ui, context));
            });
        self.windows.channel_rack = rack_open;

        let mut piano_open = self.windows.piano_roll;
        egui::Window::new("Piano Roll")
            .open(&mut piano_open)
            .default_size([1150.0, 690.0])
            .min_size([520.0, 320.0])
            .resizable(true)
            .show(context, |ui| {
                self.render_piano_roll(ui, context);
            });
        self.windows.piano_roll = piano_open;

        let mut mixer_open = self.windows.mixer;
        egui::Window::new("Mixer")
            .open(&mut mixer_open)
            .default_size([1120.0, 640.0])
            .min_size([520.0, 320.0])
            .resizable(true)
            .show(context, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.render_mixer(ui));
            });
        self.windows.mixer = mixer_open;

        let mut routing_open = self.windows.routing;
        egui::Window::new("Mixer Routing")
            .open(&mut routing_open)
            .default_size([900.0, 620.0])
            .min_size([520.0, 340.0])
            .resizable(true)
            .show(context, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.render_routing_cables(ui, context));
            });
        self.windows.routing = routing_open;

        let mut midi_open = self.windows.midi_routing;
        egui::Window::new("MIDI Routing")
            .open(&mut midi_open)
            .default_size([470.0, 600.0])
            .min_size([360.0, 300.0])
            .resizable(true)
            .show(context, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.render_midi(ui));
            });
        self.windows.midi_routing = midi_open;

        let mut automation_open = self.windows.automation;
        egui::Window::new("Plugin Automation")
            .open(&mut automation_open)
            .default_size([740.0, 560.0])
            .min_size([480.0, 320.0])
            .resizable(true)
            .show(context, |ui| self.render_automation_window(ui));
        self.windows.automation = automation_open;

        let mut samples_open = self.windows.samples;
        egui::Window::new("Sample Browser")
            .open(&mut samples_open)
            .default_size([420.0, 420.0])
            .min_size([300.0, 220.0])
            .resizable(true)
            .show(context, |ui| self.render_sample_browser(ui));
        self.windows.samples = samples_open;

        if let Some(lane) = self.renaming_track {
            let mut commit = false;
            let mut cancel = false;
            egui::Window::new("Rename track")
                .id(egui::Id::new("rename_track_window"))
                .collapsible(false)
                .resizable(false)
                .show(context, |ui| {
                    ui.label(format!(
                        "Track {} and its mixer channel share one name.",
                        lane + 1
                    ));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.rename_buffer)
                            .desired_width(260.0)
                            .hint_text("Track name"),
                    );
                    response.request_focus();
                    if response.lost_focus()
                        && context.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        commit = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Rename").clicked() {
                            commit = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if commit {
                self.commit_rename_track();
            } else if cancel {
                self.renaming_track = None;
            }
        }

        let mut settings_open = self.windows.settings;
        egui::Window::new("Settings")
            .open(&mut settings_open)
            .default_size([720.0, 560.0])
            .min_size([520.0, 340.0])
            .resizable(true)
            .show(context, |ui| self.render_settings_window(ui));
        self.windows.settings = settings_open;

        let mut render_open = self.windows.render;
        egui::Window::new("WAV Render")
            .open(&mut render_open)
            .default_size([480.0, 160.0])
            .min_size([360.0, 120.0])
            .resizable(true)
            .show(context, |ui| self.render_export_window(ui));
        self.windows.render = render_open;

        self.render_plugin_windows(context);
        self.render_debug_panel(context);

        let callback_error = match self.midi_callback_error.lock() {
            Ok(mut error) => error.take(),
            Err(_) => Some("MIDI callback error status unavailable.".to_owned()),
        };
        if let Some(error) = callback_error {
            self.status = error;
        }
        context.request_repaint_after(if self.playing {
            std::time::Duration::from_millis(8)
        } else {
            std::time::Duration::from_millis(50)
        });
    }
}

impl PulseForgeApp {
    /// Draws the debug/profiler window and hands it the live engine numbers.
    ///
    /// The panel is a separate crate so the DAW only owns the state that comes
    /// from the engine; everything else (sampling, aggregation, drawing) lives
    /// in `pulseforge-profiling`.
    fn render_debug_panel(&mut self, context: &egui::Context) {
        let (sample_rate, block_frames, dsp_load) = match &self.audio {
            Some(audio) => (audio.sample_rate, audio.block_frames(), audio.load()),
            None => (0, 0, 0.0),
        };
        let mut world = DebugWorld {
            profiler: pulseforge_profiling::profiler().map(std::convert::AsRef::as_ref),
            system: Some(&self.system_monitor),
            plugins: &mut self.plugin_debug,
            audio: AudioDebug {
                sample_rate,
                block_frames,
                dsp_load,
                playing: self.playing,
            },
        };
        self.debug_panel.show(context, &mut world);
        self.debug_summary = if self.debug_panel.is_open() {
            pulseforge_profiling::ui::summary_line(&world)
        } else {
            String::new()
        };
    }

    fn render_plugin_windows(&mut self, context: &egui::Context) {
        for instrument_id in 0..self.project.instruments.len() {
            let mut open = self.open_instrument_plugins.contains(&instrument_id);
            egui::Window::new(self.instrument_window_title(instrument_id))
                .id(egui::Id::new(("instrument_plugin_window", instrument_id)))
                .open(&mut open)
                .default_size([480.0, 360.0])
                .min_size([360.0, 240.0])
                .resizable(true)
                .show(context, |ui| {
                    self.render_instrument_plugin(instrument_id, ui)
                });
            if open {
                self.open_instrument_plugins.insert(instrument_id);
            } else {
                self.open_instrument_plugins.remove(&instrument_id);
                let host_id = pulseforge_core::hosted::instrument_host_id(instrument_id);
                if self.attached_editor == Some(host_id) {
                    self.detach_editor();
                }
            }
        }

        let open_effects: Vec<_> = self.open_effect_plugins.iter().copied().collect();
        for (channel, slot) in open_effects {
            let mut open = true;
            let channel_name = if channel == self.project.lanes.len() {
                "MASTER".to_owned()
            } else {
                self.project
                    .lanes
                    .get(channel)
                    .map_or_else(|| "Mixer".to_owned(), |lane| lane.name.clone())
            };
            let effect_name = self
                .effect_slot_name(channel, slot)
                .unwrap_or_else(|| "Empty insert".to_owned());
            egui::Window::new(format!(
                "{channel_name} · Insert {} · {effect_name}",
                slot + 1
            ))
            .id(egui::Id::new(("effect_plugin_window", channel, slot)))
            .open(&mut open)
            .default_size([420.0, 320.0])
            .min_size([320.0, 220.0])
            .resizable(true)
            .show(context, |ui| self.render_effect_plugin(channel, slot, ui));
            if !open
                || channel > self.project.lanes.len()
                || self.effect_slot_name(channel, slot).is_none()
            {
                self.open_effect_plugins.remove(&(channel, slot));
                let host_id = pulseforge_core::hosted::effect_host_id(channel, slot);
                if self.attached_editor == Some(host_id) {
                    self.detach_editor();
                }
            }
        }

        if let Some((lane, clip)) = self.sample_clip_window {
            let mut open = true;
            egui::Window::new("Sample clip")
                .id(egui::Id::new("sample_clip_window"))
                .open(&mut open)
                .default_size([560.0, 420.0])
                .min_size([420.0, 300.0])
                .resizable(true)
                .show(context, |ui| self.render_sample_clip_window(ui, lane, clip));
            if !open {
                self.sample_clip_window = None;
            }
        }

        if let Some(target) = self.plugin_selector_target {
            let mut open = true;
            egui::Window::new("Plug-in Selector")
                .id(egui::Id::new("plugin_selector_window"))
                .open(&mut open)
                .default_size([300.0, 380.0])
                .min_size([240.0, 240.0])
                .resizable(true)
                .show(context, |ui| self.render_plugin_selector(ui, target));
            if !open {
                self.plugin_selector_target = None;
            }
        }
    }
}

impl PulseForgeApp {
}

/// Static data cached per hosted plug-in, so the wrapper does not re-query the
/// bundle every frame.
struct HostedMeta {
    info: pulseforge_hosting::BundleInfo,
    parameters: Vec<pulseforge_hosting::ParameterInfo>,
    editor_size: (u32, u32),
}

/// Short clip label from a path.



#[cfg(test)]
mod tests {
    use super::*;
    use pulseforge_core::project::StepHit;
    use pulseforge_widgets::fader_math::*;

    #[test]
    fn preset_names_become_safe_file_stems() {
        assert_eq!(preset_file_stem("Lead Chain"), "Lead Chain");
        assert_eq!(preset_file_stem("a/b:c*?"), "a_b_c__");
        assert_eq!(preset_file_stem("   "), "Preset");
    }

    #[test]
    fn fader_curve_round_trips_and_snaps_to_silence() {
        assert_eq!(fader_gain(0.0), 0.0);
        assert!(fader_gain(1.0) - 1.0 < 1e-6);
        for position in [0.05, 0.25, 0.5, 0.9, 1.0] {
            let gain = fader_gain(position);
            assert!(
                (fader_position(gain) - position).abs() < 1e-3,
                "position {position} did not round-trip through {gain}"
            );
        }
        assert_eq!(fader_label(0.0), "-\u{221E} dB");
        assert!(gain_to_db(0.0) <= FADER_MIN_DB);
    }

    #[test]
    fn envelope_preview_finds_step_sequencer_hits() {
        let mut project = Project::default();
        project.patterns[0].step_hits = vec![
            StepHit {
                instrument_id: 0,
                step: 2,
            },
            StepHit {
                instrument_id: 0,
                step: 4,
            },
        ];
        project.lanes[0].clips = vec![ArrangementClip {
            start_step: 0,
            length_steps: 8,
            source: ClipSource::Pattern { pattern_id: 0 },
            offset_steps: 0,
            ..ArrangementClip::default()
        }];
        let found = automation_note_at(&project.lanes, &project.patterns, 0, 4.5);
        assert_eq!(found, Some((4.0, 1.0)));
        assert!(automation_note_at(&project.lanes, &project.patterns, 0, 1.0).is_none());
        assert!(automation_note_at(&project.lanes, &project.patterns, 1, 4.5).is_none());
    }

    #[test]
    fn automation_source_preview_reacts_to_step_hits() {
        let mut project = Project::default();
        project.patterns[0].step_hits = vec![StepHit {
            instrument_id: 0,
            step: 0,
        }];
        project.lanes[0].clips = vec![ArrangementClip {
            start_step: 0,
            length_steps: 16,
            source: ClipSource::Pattern { pattern_id: 0 },
            offset_steps: 0,
            ..ArrangementClip::default()
        }];
        let source = project::AutomationSource::MidiEnvelope {
            instrument_id: 0,
            attack: project::TimeValue {
                amount: 0.0,
                unit: project::TimeUnit::Steps,
            },
            decay: project::TimeValue {
                amount: 0.0,
                unit: project::TimeUnit::Steps,
            },
            sustain: 1.0,
            release: project::TimeValue {
                amount: 1_000.0,
                unit: project::TimeUnit::Milliseconds,
            },
            base: 0.0,
            depth: 1.0,
        };
        let target = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::Gain,
        };
        let level = automation_source_preview(
            &source,
            target,
            &project.lanes,
            &project.patterns,
            120.0,
            0,
            0.25,
        );
        assert!(level > 0.9, "envelope preview stayed at {level}");
    }

    #[test]
    fn collapsed_tracks_hide_their_automation_rows() {
        let mut project = Project::default();
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::Instrument {
                instrument_id: 0,
                parameter: InstrumentParameter::Gain,
            },
            ..AutomationTrack::default()
        });
        let expanded = arrangement_layout(&project);
        assert!(
            expanded
                .iter()
                .any(|span| matches!(span.row, ArrangementRow::Automation(0)))
        );

        project.lanes[0].automation_collapsed = true;
        let collapsed = arrangement_layout(&project);
        assert!(
            !collapsed
                .iter()
                .any(|span| matches!(span.row, ArrangementRow::Automation(0)))
        );
        assert_eq!(collapsed.len(), expanded.len() - 1);
    }

    #[test]
    fn row_heights_offset_and_hit_test_together() {
        let mut project = Project::default();
        project.lanes[0].height = 100.0;
        project.lanes[1].height = 40.0;
        let spans = arrangement_layout(&project);
        assert!((spans[0].height - 100.0).abs() < f32::EPSILON);
        assert!((spans[1].top - 100.0).abs() < f32::EPSILON);
        assert!((spans[1].height - 40.0).abs() < f32::EPSILON);
        assert_eq!(arrangement_row_at(&spans, 10.0), 0);
        assert_eq!(arrangement_row_at(&spans, 120.0), 1);
        // Past the last row the clamp keeps the index inside the layout.
        assert_eq!(arrangement_row_at(&spans, 10_000.0), spans.len() - 1);
    }

    #[test]
    fn arrangement_rows_group_automation_under_its_mixer_lane() {
        let mut project = Project::default();
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::Instrument {
                instrument_id: 0,
                parameter: InstrumentParameter::Gain,
            },
            ..AutomationTrack::default()
        });
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::InsertEffect {
                channel_id: 2,
                slot_id: 0,
                parameter: EffectParameter::Mix,
            },
            ..AutomationTrack::default()
        });
        let rows: Vec<ArrangementRow> = arrangement_layout(&project)
            .into_iter()
            .map(|span| span.row)
            .collect();
        assert!(matches!(rows[0], ArrangementRow::Lane(0)));
        assert!(matches!(rows[1], ArrangementRow::Automation(0)));
        assert!(matches!(rows[2], ArrangementRow::Lane(1)));
        assert!(matches!(rows[3], ArrangementRow::Lane(2)));
        assert!(matches!(rows[4], ArrangementRow::Automation(1)));
    }

    #[test]
    fn custom_cursor_svg_provides_shapes_for_each_tool_and_resize_target() {
        for kind in [
            "pen", "brush", "delete", "join", "pan", "resize", "move", "cut",
        ] {
            let points = svg_cursor_points(kind).unwrap();
            assert!(points.len() >= 3, "{kind} cursor has no polygon");
        }
    }

    #[test]
    fn scale_highlighting_and_snapping_follow_the_root() {
        // C major contains C and B but not C sharp.
        assert!(scale_contains_pitch(0, "Major", 60));
        assert!(scale_contains_pitch(0, "Major", 71));
        assert!(!scale_contains_pitch(0, "Major", 61));
        // Transposing the root transposes the membership: D is a whole tone
        // above C, so it is in C major but not in C sharp major.
        assert!(scale_contains_pitch(0, "Major", 62));
        assert!(!scale_contains_pitch(1, "Major", 62));
        assert!(scale_contains_pitch(1, "Major", 61));
        // Chromatic always matches, so snapping is a no-op.
        assert!(scale_contains_pitch(0, "Chromatic", 61));
        // A non-scale note snaps to the closest scale tone.
        assert_eq!(snap_pitch_into_scale(0, "Major", 61), 60);
        assert_eq!(snap_pitch_into_scale(0, "Major", 63), 62);
        assert_eq!(snap_pitch_into_scale(0, "Major", 60), 60);
    }

    #[test]
    fn alt_bypasses_the_piano_snap_setting() {
        assert_eq!(piano_snap_tick(117, 4, false), 116);
        assert_eq!(piano_snap_tick(117, 4, true), 117);
        assert_eq!(piano_snap_tick(16, 1, false), 16);
    }

    #[test]
    fn resizing_the_note_start_keeps_its_end_and_minimum_duration() {
        assert_eq!(resize_note_start(8, 4, 8, 6), (6, 6));
        assert_eq!(resize_note_start(8, 4, 8, 20), (11, 1));
        assert_eq!(resize_note_start(8, 4, 8, 0), (0, 12));
    }

    #[test]
    fn rack_grid_maps_pointer_and_paints_every_crossed_step() {
        assert_eq!(rack_step_at(48.0, 0.0, 32, 24.0, 32), 34);
        assert_eq!(rack_step_at(-10.0, 0.0, 32, 24.0, 32), 32);
        assert_eq!(rack_step_at(900.0, 0.0, 32, 24.0, 32), 63);
        assert_eq!(rack_step_span(37, 42), (37, 42));
        assert_eq!(rack_step_span(42, 37), (37, 42));
    }

    #[test]
    fn routing_helpers_interpolate_colors_and_measure_cable_distance() {
        let red = egui::Color32::from_rgb(255, 0, 0);
        let blue = egui::Color32::from_rgb(0, 0, 255);
        let middle = color_lerp(red, blue, 0.5);
        assert_eq!(middle, egui::Color32::from_rgb(128, 0, 128));
        assert!(
            (point_segment_distance(
                egui::pos2(5.0, 3.0),
                egui::pos2(0.0, 0.0),
                egui::pos2(10.0, 0.0)
            ) - 3.0)
                .abs()
                < 0.001
        );
    }
}
