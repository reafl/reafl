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
    anim, automation_bridge, context_menu, control, effect_parameters, eq_curve,
    icons, knob, lane_slider, mixer_strip, power, presets, theme,
    time_edit, tooltip,
};

use anim::Animated;
use cli::Cli;
use context_menu::{ContextAction, ContextMenu, MenuItem};
use settings::{Chord, Settings, ShortcutAction};

mod actions;
mod app_setup;
mod automation_actions;
mod automation_display;
mod clips;
mod geometry;
mod hosting;
mod input;
mod instruments;
mod labels;
mod project_io;
mod samples;
mod selection;
mod snap;
mod transport;
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




}

impl PulseForgeApp {
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
