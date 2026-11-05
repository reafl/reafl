mod anim;
mod cli;
mod context_menu;
mod settings;
mod tasks;

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, mpsc::SyncSender},
};

use anim::Animated;
use cli::Cli;
use clap::Parser;
use context_menu::{ContextAction, ContextMenu, MenuItem};
use eframe::egui;
use midir::{Ignore, MidiInput, MidiInputConnection};
use pulseforge_core::{
    audio::{self, AudioCommand, AudioOutput},
    project,
    project::{
        ArrangementClip, AutomationCurve, AutomationPoint, AutomationTrack, ClipSource,
        DefaultSample, EffectParameter, InsertFxSlot, Instrument, InstrumentParameter,
        MIDI_CHANNEL_COUNT, ParameterTarget, PianoNote, Project, SignalWaveform, SynthKind,
        ThreeBandEq, TimeSignature, disconnect_channel,
    },
};
use pulseforge_profiling::{
    Channel, PluginDebug, SystemMonitor, profile_scope, set_thread_channel,
    ui::{AudioDebug, DebugPanel, DebugWorld},
};
mod platform;

use settings::{Chord, Settings, ShortcutAction};

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

const AUTOMATED_TARGETS_KEY: &str = "pulseforge.automated_targets";
const TOOLTIP_MODE_KEY: &str = "pulseforge.tooltip_mode";
const TOOLTIP_DELAY_KEY: &str = "pulseforge.tooltip_delay";
const HOVER_TEXT_KEY: &str = "pulseforge.hover_text";
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

fn main() -> eframe::Result<()> {
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
        project::AutomationSource::Curve => 0.0,
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

    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        profile_scope!("render_toolbar");
        // Two compact icon rows: window/transport on top, tempo/grid below.
        // Every control uses the same row height so the bar stays aligned.
        const ROW_HEIGHT: f32 = 24.0;
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.spacing_mut().interact_size.y = ROW_HEIGHT;
        ui.horizontal(|ui| {
            ui.set_height(ROW_HEIGHT);
            ui.label(
                egui::RichText::new("PULSEFORGE")
                    .color(egui::Color32::from_rgb(112, 208, 190))
                    .strong(),
            );
            ui.separator();
            for (icon, hint, action) in [
                (
                    Icon::Rack,
                    "Channel rack (F6)",
                    ShortcutAction::WindowChannelRack,
                ),
                (
                    Icon::Roll,
                    "Piano roll (F7)",
                    ShortcutAction::WindowPianoRoll,
                ),
                (Icon::Mixer, "Mixer (F9)", ShortcutAction::WindowMixer),
                (
                    Icon::Routing,
                    "Routing (Shift+F9)",
                    ShortcutAction::WindowRouting,
                ),
                (
                    Icon::Automation,
                    "Automation (F10)",
                    ShortcutAction::WindowAutomation,
                ),
                (
                    Icon::Samples,
                    "Samples (F11)",
                    ShortcutAction::WindowSamples,
                ),
                (Icon::Midi, "MIDI routing (F8)", ShortcutAction::WindowMidi),
                (Icon::Settings, "Settings", ShortcutAction::WindowSettings),
                (Icon::Render, "Render WAV", ShortcutAction::WindowRender),
                (
                    Icon::Debug,
                    "Debug / profiler (Ctrl+Shift+D)",
                    ShortcutAction::WindowDebug,
                ),
            ] {
                if icon_button(ui, icon, hint).clicked() {
                    self.perform(action);
                }
            }
            ui.separator();
            if icon_button(ui, Icon::Undo, "Undo (Ctrl+Z)").clicked() {
                self.undo();
            }
            if icon_button(ui, Icon::Redo, "Redo (Ctrl+Shift+Z)").clicked() {
                self.redo();
            }
            ui.separator();
            if icon_button(
                ui,
                if self.playing { Icon::Stop } else { Icon::Play },
                "Play / stop (Space)",
            )
            .clicked()
            {
                self.perform(ShortcutAction::TransportToggle);
            }
            if icon_button(ui, Icon::Play, "Stop and rewind").clicked() {
                self.perform(ShortcutAction::TransportStop);
            }
            // Stateful transport switches show their state through the button
            // fill, so whether the loop, metronome or pattern preview is on is
            // visible without reading the status bar.
            if icon_button_toggle(ui, Icon::Loop, "Loop selection (L)", self.loop_enabled).clicked() {
                self.perform(ShortcutAction::ToggleLoop);
            }
            if icon_button_toggle(ui, Icon::Metronome, "Metronome (Ctrl+M)", self.metronome)
                .clicked()
            {
                self.perform(ShortcutAction::ToggleMetronome);
            }
            if icon_button_toggle(
                ui,
                Icon::Solo,
                "Pattern playback (Ctrl+Shift+P)",
                self.selected_pattern_playback,
            )
            .clicked()
            {
                self.perform(ShortcutAction::TogglePatternPlayback);
            }
        });
        ui.horizontal(|ui| {
            ui.set_height(ROW_HEIGHT);
            ui.label("BPM");
            let mut bpm = self.project.bpm;
            if ui
                .add_sized(
                    [58.0, ROW_HEIGHT],
                    egui::DragValue::new(&mut bpm)
                        .range(20.0..=300.0)
                        .speed(0.5),
                )
                .changed()
            {
                self.project.bpm = bpm;
                self.send_audio(AudioCommand::SetTempo(bpm));
            }
            ui.separator();
            let mut numerator = self.project.time_signature.numerator;
            let mut denominator = self.project.time_signature.denominator;
            let n = ui
                .add_sized(
                    [34.0, ROW_HEIGHT],
                    egui::DragValue::new(&mut numerator).range(1..=128),
                )
                .changed();
            ui.label("/");
            let d = ui
                .add_sized(
                    [34.0, ROW_HEIGHT],
                    egui::DragValue::new(&mut denominator).range(1..=128),
                )
                .changed();
            if n || d {
                self.project.time_signature = TimeSignature {
                    numerator,
                    denominator,
                };
                self.sync_project();
            }
            ui.separator();
            ui.label("Snap");
            let mut snap = self.project.snap_ticks;
            ui.scope(|ui| {
                // Keep the combo box inside the 24 px toolbar row.
                ui.spacing_mut().interact_size.y = 20.0;
                ui.spacing_mut().button_padding.y = 0.0;
                egui::ComboBox::from_id_salt("global_snap")
                    .selected_text(snap_label(snap))
                    .width(76.0)
                    .show_ui(ui, |ui| {
                        for ticks in SNAP_STEPS.iter().copied() {
                            ui.selectable_value(&mut snap, ticks, snap_label(ticks));
                        }
                    });
            });
            if snap != self.project.snap_ticks {
                self.project.snap_ticks = snap;
                self.sync_project();
            }
        });
    }

    fn render_clip_picker(&mut self, ui: &mut egui::Ui) {
        ui.heading("Clip picker");
        ui.label("Drag a pattern or sample to the timeline.");
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(280.0)
            .show(ui, |ui| {
                for pattern_id in 0..self.project.patterns.len() {
                    let name = self.project.patterns[pattern_id].name.clone();
                    // The selected pattern is the one the arrangement pen
                    // places, so the picker marks it.
                    let selected = pattern_id == self.selected_pattern;
                    let mut button = egui::Button::new(if selected {
                        format!("\u{25B8} [P]  {name}")
                    } else {
                        format!("[P]  {name}")
                    })
                    .sense(egui::Sense::click_and_drag());
                    if selected {
                        button = button.fill(self.ui_accent().gamma_multiply(0.35));
                    }
                    let response = ui.add_sized([ui.available_width(), 32.0], button);
                    if response.clicked() {
                        self.selected_pattern = pattern_id;
                        self.arrangement_source = Some(ClipSource::Pattern { pattern_id });
                    }
                    if response.double_clicked() {
                        self.selected_pattern = pattern_id;
                        self.windows.piano_roll = true;
                    }
                    if response.drag_started() {
                        self.pattern_drag = Some(ClipSource::Pattern { pattern_id });
                    }
                }
            });
        if ui.button("+ New pattern").clicked() {
            self.selected_pattern = self.project.add_pattern();
            self.sync_project();
        }
        ui.separator();
        ui.label("Audio clip");
        ui.add(
            egui::TextEdit::singleline(&mut self.new_audio_path)
                .desired_width(ui.available_width())
                .hint_text("WAV file path"),
        );
        ui.horizontal(|ui| {
            ui.label("Length (steps)");
            ui.add(
                egui::DragValue::new(&mut self.new_audio_length_steps)
                    .range(1..=u64::MAX)
                    .speed(1.0),
            );
        });
        let audio_label = if self.new_audio_path.is_empty() {
            "Set path to drag audio"
        } else {
            "Drag audio clip"
        };
        let audio_response = ui.add_sized(
            [ui.available_width(), 32.0],
            egui::Button::new(audio_label).sense(egui::Sense::click_and_drag()),
        );
        if audio_response.drag_started() && !self.new_audio_path.trim().is_empty() {
            self.pattern_drag = Some(ClipSource::Audio {
                path: self.new_audio_path.trim().to_owned(),
            });
        }
        if audio_response.clicked() && !self.new_audio_path.trim().is_empty() {
            self.arrangement_source = Some(ClipSource::Audio {
                path: self.new_audio_path.trim().to_owned(),
            });
            self.add_audio_clip(self.arrangement_pan_steps, self.selected_lane);
        }
        ui.add_space(12.0);
        if ui.button("+ Arrangement lane").clicked() {
            let old_master_id = self.project.lanes.len();
            self.project.add_lane();
            if self.automation_channel_id == old_master_id {
                self.automation_channel_id = old_master_id + 1;
            }
            self.selected_lane = self.project.lanes.len() - 1;
            self.sync_project();
        }
    }

    fn render_arrangement(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        profile_scope!("render_arrangement");
        // The arrangement keeps its own tool so piano-roll choices cannot place
        // clips, and Join (note-only) never becomes the arrangement tool.
        if self.arrangement_tool == PianoTool::Join {
            self.arrangement_tool = PianoTool::Pen;
        }
        ui.horizontal(|ui| {
            ui.label("Arrangement");
            ui.separator();
            tool_palette(ui, &mut self.arrangement_tool, false);
            ui.separator();
            ui.label(format!(
                "{}/{} · {} steps/bar",
                self.project.time_signature.numerator,
                self.project.time_signature.denominator,
                self.project.time_signature.steps_per_bar()
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    "Pen places · Brush repeats · Cut slices · Alt disables snap · Middle-drag pans",
                );
            });
        });
        ui.separator();
        self.render_timeline(ui, context);
    }

    fn render_automation_arrangement_row(
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
        if primary_pressed
            && let Some(position) = pointer.filter(|position| row.contains(*position))
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
        let project = &mut self.project;
        let lanes = &project.lanes;
        let patterns = &project.patterns;
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
        painter.line_segment(
            [
                egui::pos2(grid.grid_left, row.bottom()),
                egui::pos2(row.right(), row.bottom()),
            ],
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(91, 67, 110)),
        );
        // Right-clicking the automation clip offers duplication so one
        // parameter can carry several clips side by side.
        let clip_response = ui.interact(
            clip_rect,
            egui::Id::new(("automation_clip", track_id)),
            egui::Sense::click(),
        );
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
        let mut picked_action = None;
        clip_response.context_menu(|ui| picked_action = automation_menu.show(ui));
        if let Some(action) = picked_action {
            self.apply_context_action(action);
            return false;
        }
        let multiplier_points = track.modulation_points.clone();
        let curve_source = matches!(track.source, project::AutomationSource::Curve);
        let points = if curve_source {
            &mut track.points
        } else {
            &mut track.modulation_points
        };
        let point_position = |point: &AutomationPoint| {
            egui::pos2(
                grid.grid_left
                    + (point.step as f64 - grid.start_step as f64) as f32 * grid.step_width,
                row.bottom() - 6.0 - point.value.clamp(0.0, 1.0) * (row.height() - 12.0),
            )
        };
        if curve_source {
            let mut curve = Vec::new();
            // Samples sit on a fixed quarter-step grid instead of pixel columns,
            // so zooming shifts the curve continuously instead of jittering.
            let sample_count = ((visible_span_steps / 0.25).ceil() as usize).clamp(2, 512);
            for index in 0..=sample_count {
                let x = clip_start + clip_rect.width() * index as f32 / sample_count as f32;
                let step = grid.start_step as f64
                    + f64::from(x - grid.grid_left) / f64::from(grid.step_width);
                let value = Project::evaluate_curve(points, step);
                curve.push(egui::pos2(
                    x,
                    row.bottom() - 6.0 - value.clamp(0.0, 1.0) * (row.height() - 12.0),
                ));
            }
            painter.add(egui::Shape::line(
                curve,
                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(202, 158, 238)),
            ));
        } else {
            let source = track.source.clone();
            let target = track.target;
            let sample_count = ((visible_span_steps / 0.25).ceil() as usize).clamp(2, 768);
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
                let value = automation_source_preview(
                    &source,
                    target,
                    lanes,
                    patterns,
                    bpm,
                    track.start_step,
                    step,
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
        for (point_id, point) in points.iter_mut().enumerate() {
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
                point.value =
                    (1.0 - (pointer.y - row.top() - 6.0) / (row.height() - 12.0)).clamp(0.0, 1.0);
                changed = true;
            }
            response.context_menu(|ui| {
                if curve_source {
                    ui.label("Curve to this node");
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
            painter.circle_filled(
                point_position(point),
                4.5,
                if curve_source {
                    egui::Color32::from_rgb(229, 185, 251)
                } else {
                    egui::Color32::from_rgb(255, 218, 112)
                },
            );
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
        if response.clicked() {
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
                    let value = (1.0 - (pointer.y - row.top() - 6.0) / (row.height() - 12.0))
                        .clamp(0.0, 1.0);
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

    fn render_timeline(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
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

    fn render_channel_rack(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        profile_scope!("render_channel_rack");
        ui.horizontal(|ui| {
            ui.label("Pattern");
            let pattern_name = &self.project.patterns[self.selected_pattern].name;
            egui::ComboBox::from_id_salt("channel_pattern")
                .selected_text(pattern_name)
                .show_ui(ui, |ui| {
                    for (id, pattern) in self.project.patterns.iter().enumerate() {
                        ui.selectable_value(&mut self.selected_pattern, id, &pattern.name);
                    }
                });
            ui.label("Length (steps)");
            let mut length = self.project.patterns[self.selected_pattern].length_steps;
            if ui
                .add(
                    egui::DragValue::new(&mut length)
                        .range(1..=u64::MAX)
                        .speed(1.0),
                )
                .changed()
            {
                self.project.patterns[self.selected_pattern].length_steps = length.max(1);
                self.sync_project();
            }
            if ui.button("+ Instrument").clicked() {
                let id = self.project.instruments.len();
                self.project.instruments.push(Instrument {
                    name: format!("Instrument {}", id + 1),
                    default_mixer_lane: self.selected_lane.min(self.project.lanes.len() - 1),
                    ..Instrument::default()
                });
                self.selected_instrument = id;
                self.selection = Some(Selection::Instrument(id));
                self.sync_project();
            }
            ui.label(format!(
                "Page {} · Middle-drag to pan",
                self.pattern_pan_steps / VISIBLE_PATTERN_STEPS + 1
            ));
        });
        ui.separator();

        let start = self.pattern_pan_steps;
        let end = start.saturating_add(VISIBLE_PATTERN_STEPS);
        let instrument_count = self.project.instruments.len();
        for instrument_id in 0..instrument_count {
            ui.horizontal(|ui| {
                // Name and plug-in button are one control: clicking toggles the
                // plug-in window, right-clicking opens the channel menu.
                let instrument_color = self.instrument_color(instrument_id);
                let (enabled, locked, solo) = {
                    let item = &self.project.instruments[instrument_id];
                    (item.enabled, item.locked, item.solo)
                };
                let is_open = self.open_instrument_plugins.contains(&instrument_id);
                let rename_here = self.renaming_instrument == Some(instrument_id);
                if rename_here {
                    let buffer = &mut self.rename_buffer;
                    let response = ui.add_sized(
                        [110.0, 25.0],
                        egui::TextEdit::singleline(buffer).hint_text("Name"),
                    );
                    response.request_focus();
                    let (enter, escape) = ui.input(|input| {
                        (
                            input.key_pressed(egui::Key::Enter),
                            input.key_pressed(egui::Key::Escape),
                        )
                    });
                    if enter {
                        self.commit_instrument_rename();
                    } else if escape {
                        self.renaming_instrument = None;
                    }
                } else {
                    let label = egui::RichText::new(&self.project.instruments[instrument_id].name)
                        .color(instrument_color);
                    let response = ui.add_sized(
                        [110.0, 25.0],
                        egui::Button::new(label)
                            .fill(if is_open {
                                instrument_color.gamma_multiply(0.35)
                            } else {
                                egui::Color32::from_gray(32)
                            })
                            .sense(egui::Sense::click_and_drag()),
                    );
                    if response.clicked() {
                        self.selected_instrument = instrument_id;
                        self.selection = Some(Selection::Instrument(instrument_id));
                        if is_open {
                            self.open_instrument_plugins.remove(&instrument_id);
                        } else {
                            self.open_instrument_plugins.insert(instrument_id);
                        }
                    }
                    if response.drag_started() {
                        self.pattern_drag = None;
                    }
                    let channel_menu = ContextMenu {
                        items: vec![
                            MenuItem::Action {
                                label: "Rename".to_owned(),
                                action: ContextAction::RenameInstrument(instrument_id),
                            },
                            MenuItem::Action {
                                label: "Remove".to_owned(),
                                action: ContextAction::RemoveInstrument(instrument_id),
                            },
                            MenuItem::Folder {
                                label: "Colour".to_owned(),
                                items: vec![
                                    MenuItem::Action {
                                        label: "Default".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: instrument_id,
                                            color: None,
                                        },
                                    },
                                    MenuItem::Action {
                                        label: "Teal".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: instrument_id,
                                            color: Some([112, 208, 190]),
                                        },
                                    },
                                    MenuItem::Action {
                                        label: "Amber".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: instrument_id,
                                            color: Some([230, 178, 92]),
                                        },
                                    },
                                    MenuItem::Action {
                                        label: "Rose".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: instrument_id,
                                            color: Some([226, 120, 140]),
                                        },
                                    },
                                    MenuItem::Action {
                                        label: "Violet".to_owned(),
                                        action: ContextAction::SetInstrumentColor {
                                            instrument: instrument_id,
                                            color: Some([160, 140, 230]),
                                        },
                                    },
                                ],
                            },
                            MenuItem::Separator,
                            MenuItem::Action {
                                label: "Choose plug-in?".to_owned(),
                                action: ContextAction::OpenInstrumentPicker(instrument_id),
                            },
                        ],
                    };
                    let mut channel_action = None;
                    response.context_menu(|ui| channel_action = channel_menu.show(ui));
                    if let Some(action) = channel_action {
                        self.apply_context_action(action);
                    }
                }
                // One power switch; shift locks, ctrl solos.
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(18.0, 20.0), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    3.0,
                    if enabled {
                        instrument_color.gamma_multiply(0.45)
                    } else {
                        egui::Color32::from_gray(30)
                    },
                );
                paint_icon(
                    ui.painter(),
                    rect.shrink(3.0),
                    Icon::Power,
                    if enabled {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::from_gray(110)
                    },
                );
                if locked {
                    ui.painter().line_segment(
                        [
                            egui::pos2(rect.left() + 2.0, rect.bottom() - 2.0),
                            egui::pos2(rect.right() - 2.0, rect.bottom() - 2.0),
                        ],
                        egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(230, 178, 92)),
                    );
                }
                if solo {
                    ui.painter().circle_filled(
                        egui::pos2(rect.right() - 3.0, rect.top() + 3.0),
                        2.5,
                        egui::Color32::from_rgb(120, 200, 255),
                    );
                }
                tooltip(ui, &response, "Power - shift locks, ctrl solos");
                if response.clicked() {
                    self.toggle_instrument_switch(instrument_id);
                }
                let instrument = &mut self.project.instruments[instrument_id];
                let mut gain = instrument.gain;
                let mut pan = instrument.pan;
                let gain_target = ParameterTarget::Instrument {
                    instrument_id,
                    parameter: InstrumentParameter::Gain,
                };
                let pan_target = ParameterTarget::Instrument {
                    instrument_id,
                    parameter: InstrumentParameter::Pan,
                };
                let gain_changed = Knob::new("VOL", 0.0, 1.0, "")
                    .compact()
                    .with_default(0.8)
                    .automates(gain_target)
                    .show(ui, &mut gain);
                let pan_changed = Knob::new("PAN", -1.0, 1.0, "")
                    .compact()
                    .with_default(0.0)
                    .automates(pan_target)
                    .show(ui, &mut pan);
                if gain_changed {
                    self.project.instruments[instrument_id].gain = gain;
                    self.sync_project();
                }
                if pan_changed {
                    self.project.instruments[instrument_id].pan = pan;
                    self.sync_project();
                }
                let pattern_id = self.selected_pattern;
                let pattern_length = self.project.patterns[pattern_id].length_steps;
                // A fixed column keeps every row aligned; the rack window's scroll
                // area handles overflow instead of squeezing the steps.
                let step_width = STEP_COLUMN_WIDTH;
                let grid_width = step_width * VISIBLE_PATTERN_STEPS as f32;
                let (grid_rect, grid_response) = ui.allocate_exact_size(
                    egui::vec2(grid_width, 26.0),
                    egui::Sense::click_and_drag(),
                );
                let painter = ui.painter_at(grid_rect);
                for step in start..end {
                    let active = self.project.has_step_note(pattern_id, instrument_id, step);
                    let color = if step >= pattern_length {
                        egui::Color32::from_gray(19)
                    } else if active {
                        lane_color(instrument_id)
                    } else if step % 4 == 0 {
                        egui::Color32::from_gray(62)
                    } else {
                        egui::Color32::from_gray(38)
                    };
                    let step_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            grid_rect.left() + (step - start) as f32 * step_width,
                            grid_rect.top(),
                        ),
                        egui::vec2(step_width, 26.0),
                    );
                    painter.rect_filled(step_rect.shrink(1.0), 3.0, color);
                    let marker_color = if active {
                        egui::Color32::from_gray(245)
                    } else {
                        egui::Color32::from_gray(105)
                    };
                    painter.circle_filled(
                        step_rect.center(),
                        if active { 3.0 } else { 1.5 },
                        marker_color,
                    );
                }
                let (primary_pressed, secondary_pressed, primary_down, secondary_down) = context
                    .input(|input| {
                        (
                            input.pointer.button_pressed(egui::PointerButton::Primary),
                            input.pointer.button_pressed(egui::PointerButton::Secondary),
                            input.pointer.button_down(egui::PointerButton::Primary),
                            input.pointer.button_down(egui::PointerButton::Secondary),
                        )
                    });
                let pointer = context.input(|input| input.pointer.interact_pos());
                if let Some(position) = pointer.filter(|position| {
                    grid_rect.contains(*position)
                        && (grid_response.contains_pointer()
                            || self.rack_gesture.is_some()
                            || self.pattern_drag.is_some())
                }) {
                    let step = rack_step_at(
                        position.x,
                        grid_rect.left(),
                        start,
                        step_width,
                        VISIBLE_PATTERN_STEPS,
                    );
                    if primary_pressed || secondary_pressed {
                        let enabled = primary_pressed;
                        self.selected_instrument = instrument_id;
                        self.selection = Some(Selection::Instrument(instrument_id));
                        self.set_pattern_hit_for(pattern_id, instrument_id, step, enabled);
                        self.rack_gesture = Some(RackGesture {
                            pattern_id,
                            instrument_id,
                            enabled,
                            last_step: step,
                        });
                    } else if (primary_down || secondary_down)
                        && grid_response.hovered()
                        && let Some(mut gesture) = self.rack_gesture
                        && gesture.pattern_id == pattern_id
                        && gesture.instrument_id == instrument_id
                        && gesture.last_step != step
                    {
                        let (first_step, last_step) = rack_step_span(gesture.last_step, step);
                        for painted_step in first_step..=last_step {
                            self.set_pattern_hit_for(
                                pattern_id,
                                instrument_id,
                                painted_step,
                                gesture.enabled,
                            );
                        }
                        gesture.last_step = step;
                        self.rack_gesture = Some(gesture);
                    }
                }
                if ui.small_button("Piano").clicked() {
                    self.selected_instrument = instrument_id;
                    self.selection = Some(Selection::Instrument(instrument_id));
                    self.windows.piano_roll = true;
                }
            });
        }
        let drag_finished = context.input(|input| {
            !input.pointer.button_down(egui::PointerButton::Primary)
                && !input.pointer.button_down(egui::PointerButton::Secondary)
        });
        if drag_finished && self.rack_gesture.take().is_some() {
            self.sync_project();
        }

        let rect = ui.available_rect_before_wrap();
        context.input(|input| {
            if input.pointer.button_down(egui::PointerButton::Middle)
                && rect.contains(input.pointer.interact_pos().unwrap_or_default())
                && input.pointer.delta().x != 0.0
            {
                self.pattern_pan_steps = self
                    .pattern_pan_steps
                    .saturating_add_signed((-input.pointer.delta().x / 22.0).round() as i64);
            }
            // The wheel scrolls across steps as well, which is what the rack was
            // missing: only middle-drag used to pan.
            let scroll = input.raw_scroll_delta.y;
            if scroll.abs() > 0.01 && rect.contains(input.pointer.interact_pos().unwrap_or_default())
            {
                let direction = if scroll > 0.0 { -2 } else { 2 };
                self.pattern_pan_steps = self.pattern_pan_steps.saturating_add_signed(direction);
            }
        });
        ui.label("Left-drag paints steps on; right-drag paints off. Patterns are sparse and have no fixed step limit.");
    }

    /// Toggles one step of the step sequencer.
    ///
    /// Steps are real C5 notes, which is what the rack draws and what the piano
    /// roll shows; writing `step_hits` instead left the buttons looking dead.
    fn set_pattern_hit_for(
        &mut self,
        pattern_id: usize,
        instrument_id: usize,
        step: u64,
        enabled: bool,
    ) -> bool {
        let velocity = self.settings.default_velocity.clamp(1, 127);
        let pattern = &mut self.project.patterns[pattern_id];
        if enabled && step >= pattern.length_steps {
            // Activating a step outside the range grows the pattern, so the range
            // always covers what the user painted.
            pattern.length_steps = step + 1;
        }
        let existing = pattern.notes.iter().position(|note| {
            note.instrument_id == instrument_id
                && note.pitch == project::STEP_SEQUENCER_PITCH
                && note.start_step == step
        });
        match (enabled, existing) {
            (true, None) => {
                let mut note = PianoNote {
                    instrument_id,
                    pitch: project::STEP_SEQUENCER_PITCH,
                    duration_steps: 1,
                    velocity,
                    ..PianoNote::default()
                };
                note.set_start_tick(step.saturating_mul(16));
                note.set_duration_ticks(16);
                pattern.notes.push(note);
                true
            }
            (false, Some(index)) => {
                pattern.notes.remove(index);
                true
            }
            _ => false,
        }
    }

    fn render_piano_roll(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        profile_scope!("render_piano_roll");
        egui::Frame::NONE.inner_margin(4.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Pattern");
                let name = &self.project.patterns[self.selected_pattern].name;
                egui::ComboBox::from_id_salt("piano_pattern")
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for (id, pattern) in self.project.patterns.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_pattern, id, &pattern.name);
                        }
                    });
                ui.label("Instrument");
                egui::ComboBox::from_id_salt("piano_instrument")
                    .selected_text(&self.project.instruments[self.selected_instrument].name)
                    .show_ui(ui, |ui| {
                        for (id, instrument) in self.project.instruments.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_instrument,
                                id,
                                &instrument.name,
                            );
                        }
                    });
                ui.label(format!("Snap: {}", snap_label(self.project.snap_ticks)));
                ui.separator();
                // Scale highlighting and note snapping.
                ui.label("Scale");
                egui::ComboBox::from_id_salt("piano_scale")
                    .selected_text(&self.settings.scale_name)
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for name in Self::SCALE_NAMES {
                            ui.selectable_value(
                                &mut self.settings.scale_name,
                                name.to_owned(),
                                name,
                            );
                        }
                    });
                ui.label("Root");
                let mut root = self.settings.scale_root % 12;
                egui::ComboBox::from_id_salt("piano_scale_root")
                    .selected_text(note_name(root))
                    .width(58.0)
                    .show_ui(ui, |ui| {
                        for value in 0..12u8 {
                            ui.selectable_value(&mut root, value, note_name(value));
                        }
                    });
                self.settings.scale_root = root;
                ui.checkbox(&mut self.settings.scale_snap, "Snap to scale");
                ui.separator();
                ui.label("Length");
                ui.add(
                    egui::DragValue::new(&mut self.note_length_steps)
                        .range(1..=u64::MAX)
                        .speed(1.0),
                );
                ui.separator();
                ui.label("New notes");
                ui.add(
                    egui::DragValue::new(&mut self.note_repeat_count)
                        .range(0..=32)
                        .speed(0.1),
                )
                .on_hover_text("Extra triggers for newly drawn notes");
                ui.label("Every");
                ui.add(
                    egui::DragValue::new(&mut self.note_repeat_ticks)
                        .range(0..=64)
                        .speed(0.2),
                )
                .on_hover_text("Ticks between repeats (0 = half the note length)");
                ui.label("Decay");
                ui.add(
                    egui::DragValue::new(&mut self.note_repeat_decay)
                        .range(0..=126)
                        .speed(0.5),
                )
                .on_hover_text("Velocity lost per repeat");
                if ui
                    .button("Apply to selected")
                    .on_hover_text("Apply the repeat settings to the selected notes")
                    .clicked()
                {
                    self.apply_repeat_to_selection();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Control");
                for (lane, label, hint) in [
                    (
                        ControlLane::Velocity,
                        "Velocity",
                        "Velocity lane (Ctrl+Shift+V)",
                    ),
                    (
                        ControlLane::Repeats,
                        "Repeats",
                        "Repeats lane (Ctrl+Shift+R)",
                    ),
                ] {
                    if ui
                        .selectable_label(self.control_lane == lane, label)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        self.control_lane = lane;
                    }
                }
                if ui
                    .selectable_label(!self.control_lane_collapsed, "Lane")
                    .on_hover_text("Show or hide the control lane")
                    .clicked()
                {
                    self.control_lane_collapsed = !self.control_lane_collapsed;
                }
                ui.separator();
                let selected_notes = self.selected_note_count();
                ui.label(format!("Sel {selected_notes}"));
                if ui
                    .small_button("All")
                    .on_hover_text("Select every note")
                    .clicked()
                {
                    self.select_all_notes();
                }
                if ui.small_button("Clear").clicked() {
                    self.clear_selection();
                }
                if ui
                    .small_button("Range")
                    .on_hover_text("Ctrl+click two notes to select everything between them")
                    .clicked()
                {
                    self.status =
                        "Ctrl+click the first and last note to select the range.".to_owned();
                }
                ui.separator();
                tool_palette(ui, &mut self.piano_tool, true);
                ui.label("Right-drag deletes · Middle-drag pans · Alt disables snap");
            });
            ui.separator();
        });
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(ui, |ui| self.piano_grid(ui, context));
    }

    fn piano_grid(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let visible_steps = self.piano_visible_steps();
        let step_width = ((ui.available_width() - 54.0) / visible_steps as f32)
            .clamp(PIANO_MIN_STEP_WIDTH, PIANO_MAX_STEP_WIDTH)
            * self.settings.zoom;
        let row_height = (ui
            .available_height()
            .max(PIANO_MIN_ROW_HEIGHT * PIANO_ROWS as f32)
            / PIANO_ROWS as f32)
            .clamp(PIANO_MIN_ROW_HEIGHT, PIANO_MAX_ROW_HEIGHT);
        let width = visible_steps as f32 * step_width;
        let height = PIANO_ROWS as f32 * row_height;
        // The control lane is pinned under the grid; collapsing it hands the
        // space back to the notes.
        let lane_height = if self.control_lane_collapsed {
            0.0
        } else {
            VELOCITY_LANE_HEIGHT
        };
        let (ruler_rect, keys_rect, grid_rect, response, velocity_rect, velocity_response) = ui
            .vertical(|ui| {
                let (ruler_rect, _) =
                    ui.allocate_exact_size(egui::vec2(54.0 + width, 14.0), egui::Sense::hover());
                let (keys_rect, grid_rect, response) = ui
                    .horizontal(|ui| {
                        let (keys_rect, _) =
                            ui.allocate_exact_size(egui::vec2(54.0, height), egui::Sense::hover());
                        let (grid_rect, response) = ui.allocate_exact_size(
                            egui::vec2(width, height),
                            egui::Sense::click_and_drag(),
                        );
                        (keys_rect, grid_rect, response)
                    })
                    .inner;
                let (velocity_rect, velocity_response) = ui
                    .horizontal(|ui| {
                        ui.allocate_exact_size(
                            egui::vec2(54.0, lane_height),
                            egui::Sense::hover(),
                        );
                        ui.allocate_exact_size(
                            egui::vec2(width, lane_height),
                            egui::Sense::click_and_drag(),
                        )
                    })
                    .inner;
                (
                    ruler_rect,
                    keys_rect,
                    grid_rect,
                    response,
                    velocity_rect,
                    velocity_response,
                )
            })
            .inner;
        let painter = ui.painter_at(grid_rect);
        let ruler_painter = ui.painter_at(ruler_rect);
        let absolute_start = self.piano_pan_steps;
        let visible_end = absolute_start.saturating_add(visible_steps);
        let pattern = &self.project.patterns[self.selected_pattern];

        for row in 0..PIANO_ROWS {
            let pitch = PIANO_MIN_PITCH + (PIANO_ROWS - 1 - row) as u8;
            let y = grid_rect.top() + row as f32 * row_height;
            let rect = egui::Rect::from_min_size(
                egui::pos2(grid_rect.left(), y),
                egui::vec2(grid_rect.width(), row_height),
            );
            let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            // Rows outside the selected scale are dimmed, so the usable notes
            // are obvious at a glance.
            let in_scale = self.scale_contains(pitch);
            let base = if black {
                egui::Color32::from_gray(30)
            } else {
                egui::Color32::from_gray(38)
            };
            painter.rect_filled(
                rect,
                0.0,
                if in_scale {
                    base
                } else {
                    base.gamma_multiply(0.72)
                },
            );
            let key_rect = egui::Rect::from_min_size(
                egui::pos2(keys_rect.left(), y),
                egui::vec2(keys_rect.width(), row_height),
            );
            ui.painter().rect_filled(
                key_rect,
                1.0,
                if black {
                    egui::Color32::from_gray(55)
                } else {
                    egui::Color32::from_gray(192)
                },
            );
            // The scale root is marked on the keys, so the highlighted rows have
            // a visible reference note.
            if pitch % 12 == self.settings.scale_root % 12 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        key_rect.left_top(),
                        egui::vec2(3.0, key_rect.height()),
                    ),
                    0.0,
                    self.ui_accent(),
                );
            }
            if pitch % 12 == 0 {
                ui.painter().text(
                    key_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    note_name(pitch),
                    egui::FontId::proportional(10.0),
                    egui::Color32::BLACK,
                );
            }
        }
        for step in 0..=visible_steps {
            let x = grid_rect.left() + step as f32 * step_width;
            painter.line_segment(
                [
                    egui::pos2(x, grid_rect.top()),
                    egui::pos2(x, grid_rect.bottom()),
                ],
                egui::Stroke::new(
                    if (absolute_start + step) % 4 == 0 {
                        1.2
                    } else {
                        0.5_f32
                    },
                    if (absolute_start + step) % 4 == 0 {
                        egui::Color32::from_gray(100)
                    } else {
                        egui::Color32::from_gray(58)
                    },
                ),
            );
            if step < visible_steps {
                ruler_painter.text(
                    egui::pos2(x + step_width * 0.5, ruler_rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    (absolute_start + step).to_string(),
                    egui::FontId::proportional(9.0),
                    if (absolute_start + step) % 4 == 0 {
                        egui::Color32::from_gray(215)
                    } else {
                        egui::Color32::from_gray(140)
                    },
                );
            }
        }
        for note in pattern.notes.iter().filter(|note| {
            note.instrument_id == self.selected_instrument
                && note.start_step < visible_end
                && note.start_step.saturating_add(note.duration_steps) > absolute_start
                && note.pitch >= PIANO_MIN_PITCH
                && usize::from(note.pitch - PIANO_MIN_PITCH) < PIANO_ROWS
        }) {
            let row = PIANO_ROWS - 1 - usize::from(note.pitch - PIANO_MIN_PITCH);
            let note_start = note.start_tick() as f32 / 16.0;
            let note_end = note.start_tick().saturating_add(note.duration_ticks()) as f32 / 16.0;
            let x = grid_rect.left() + (note_start - absolute_start as f32) * step_width;
            let right = grid_rect.left() + (note_end - absolute_start as f32) * step_width;
            let rect = egui::Rect::from_min_max(
                egui::pos2(
                    x.max(grid_rect.left()) + 1.0,
                    grid_rect.top() + row as f32 * row_height + 2.0,
                ),
                egui::pos2(
                    right.min(grid_rect.right()) - 1.0,
                    grid_rect.top() + (row + 1) as f32 * row_height - 2.0,
                ),
            );
            painter.rect_filled(rect, 3.0, lane_color(self.selected_instrument));
            if self.is_selected(Selection::Note {
                pattern: self.selected_pattern,
                instrument: note.instrument_id,
                pitch: note.pitch,
                start_step: note.start_step,
            }) {
                painter.rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2((rect.right() - 5.0).max(rect.left()), rect.top()),
                    rect.right_bottom(),
                ),
                2.0,
                egui::Color32::WHITE.gamma_multiply(0.35),
            );
            // Repeats are drawn as thin dividers so a repeated note shows where
            // each retrigger lands without leaving the piano roll.
            if note.repeat_count > 0 {
                let spacing = if note.repeat_ticks == 0 {
                    note.duration_ticks().max(1)
                } else {
                    u64::from(note.repeat_ticks) * 4
                };
                let marker = egui::Stroke::new(1.0_f32, egui::Color32::WHITE.gamma_multiply(0.6));
                for repeat in 1..=u64::from(note.repeat_count) {
                    let repeat_tick = note.start_tick() + spacing * repeat;
                    let marker_x = grid_rect.left()
                        + (repeat_tick as f32 / 16.0 - absolute_start as f32) * step_width;
                    if marker_x > rect.left() + 1.0 && marker_x < rect.right() - 1.0 {
                        painter.line_segment(
                            [egui::pos2(marker_x, rect.top() + 2.0), egui::pos2(marker_x, rect.bottom() - 2.0)],
                            marker,
                        );
                    }
                }
            }
        }

        self.handle_piano_input(grid_rect, response, context, step_width, row_height);
        if !self.control_lane_collapsed {
            self.velocity_lane(
                ui,
                context,
                velocity_rect,
                velocity_response,
                step_width,
                absolute_start,
            );
        }
        if self.playing {
            let position = self.playhead_step();
            let x = grid_rect.left() + (position - absolute_start as f64) as f32 * step_width;
            if x >= grid_rect.left() && x <= grid_rect.right() {
                painter.line_segment(
                    [
                        egui::pos2(x, grid_rect.top()),
                        egui::pos2(x, grid_rect.bottom()),
                    ],
                    egui::Stroke::new(1.6_f32, egui::Color32::from_rgb(255, 104, 79)),
                );
                ruler_painter.circle_filled(
                    egui::pos2(x, ruler_rect.center().y),
                    3.5,
                    egui::Color32::from_rgb(255, 104, 79),
                );
            }
        }
        // Dragging across the ruler selects every note that starts inside the
        // range. Unlike the arrangement loop this never affects playback.
        let ruler_response = ui.interact(
            ruler_rect,
            egui::Id::new("piano_ruler_select"),
            egui::Sense::click_and_drag(),
        );
        let ruler_step = |x: f32| -> u64 {
            let local = ((x - grid_rect.left()) / step_width).max(0.0);
            absolute_start.saturating_add(local as u64)
        };
        if ruler_response.drag_started() {
            if let Some(pointer) = context.input(|input| input.pointer.interact_pos()) {
                self.piano_select_start = Some(ruler_step(pointer.x));
            }
        }
        if ruler_response.dragged()
            && let (Some(start), Some(pointer)) = (
                self.piano_select_start,
                context.input(|input| input.pointer.interact_pos()),
            )
        {
            let end = ruler_step(pointer.x);
            self.piano_selection = Some((start.min(end), start.max(end).saturating_add(1)));
        }
        if ruler_response.drag_stopped()
            && let Some((start, end)) = self.piano_selection
        {
            self.select_notes_in_range(start, end);
            self.piano_select_start = None;
        }
        if let Some((start, end)) = self.piano_selection {
            let x0 = grid_rect.left() + (start.saturating_sub(absolute_start)) as f32 * step_width;
            let x1 = grid_rect.left() + (end.saturating_sub(absolute_start)) as f32 * step_width;
            ruler_painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x0, ruler_rect.top() + 1.0),
                    egui::pos2(x1.max(x0 + 2.0), ruler_rect.bottom() - 1.0),
                ),
                2.0,
                egui::Color32::from_rgb(112, 208, 190),
            );
        }
        // Shows the Ctrl+drag rubber band while it is being dragged.
        if let Some((origin_tick, origin_pitch)) = self.piano_box_origin
            && let Some(pointer) = context.input(|input| input.pointer.interact_pos())
        {
            let row_of = |pitch: u8| {
                let offset = pitch
                    .saturating_sub(PIANO_MIN_PITCH)
                    .min((PIANO_ROWS - 1) as u8);
                (PIANO_ROWS - 1 - usize::from(offset)) as f32
            };
            let x0 = grid_rect.left()
                + (origin_tick as f32 / 16.0 - absolute_start as f32) * step_width;
            let y0 = grid_rect.top() + row_of(origin_pitch) * row_height;
            let pointer_row = ((pointer.y - grid_rect.top()) / row_height)
                .floor()
                .clamp(0.0, (PIANO_ROWS - 1) as f32);
            let y1 = grid_rect.top() + (pointer_row + 1.0) * row_height;
            let band = egui::Rect::from_two_pos(
                egui::pos2(x0, y0),
                egui::pos2(
                    pointer.x.clamp(grid_rect.left(), grid_rect.right()),
                    y1,
                ),
            );
            if band.width() > 2.0 || band.height() > 2.0 {
                painter.rect_filled(
                    band,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(112, 208, 190, 40),
                );
            }
        }
        self.paint_tool_cursor(ui, context, grid_rect, step_width, row_height);
    }

    /// Selects every note whose start lies inside the rubber band.
    fn select_notes_in_box(&mut self, from_tick: u64, to_tick: u64, from_pitch: u8, to_pitch: u8) {
        let (low_tick, high_tick) = (from_tick.min(to_tick), from_tick.max(to_tick));
        let (low_pitch, high_pitch) = (from_pitch.min(to_pitch), from_pitch.max(to_pitch));
        let instrument = self.selected_instrument;
        let hits: Vec<Selection> = self.project.patterns[self.selected_pattern]
            .notes
            .iter()
            .filter(|note| {
                // A note is selected when the band overlaps its body, not only
                // when it starts inside, which is what users expect from a box.
                let start = note.start_tick();
                let end = start.saturating_add(note.duration_ticks().max(1));
                note.instrument_id == instrument
                    && end > low_tick
                    && start <= high_tick
                    && note.pitch >= low_pitch
                    && note.pitch <= high_pitch
            })
            .map(|note| Selection::Note {
                pattern: self.selected_pattern,
                instrument: note.instrument_id,
                pitch: note.pitch,
                start_step: note.start_step,
            })
            .collect();
        if let Some(first) = hits.first().copied() {
            self.selection = Some(first);
        }
        self.extra_selection = hits.iter().skip(1).copied().collect();
        self.selection_anchor = hits.first().copied();
        self.status = format!("{} note(s) in the box.", hits.len());
    }

    /// Selects every note starting inside `[start, end)` steps.
    fn select_notes_in_range(&mut self, start: u64, end: u64) {
        let Some(pattern) = self.project.patterns.get(self.selected_pattern) else {
            return;
        };
        let hits: Vec<Selection> = pattern
            .notes
            .iter()
            .filter(|note| note.start_step >= start && note.start_step < end)
            .map(|note| Selection::Note {
                pattern: self.selected_pattern,
                instrument: note.instrument_id,
                pitch: note.pitch,
                start_step: note.start_step,
            })
            .collect();
        self.selection = hits.first().copied();
        self.extra_selection = hits.iter().skip(1).copied().collect();
        self.selection_anchor = hits.first().copied();
        self.status = format!("Selected {} note(s) in the ruler range.", hits.len());
    }

    /// Velocity editor strip below the piano grid: drag over a note's column to
    /// change how hard that note is played.
    fn velocity_lane(
        &mut self,
        ui: &mut egui::Ui,
        context: &egui::Context,
        rect: egui::Rect,
        response: egui::Response,
        step_width: f32,
        absolute_start: u64,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(26));
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(45)),
        );
        let repeats_mode = self.control_lane == ControlLane::Repeats;
        painter.text(
            egui::pos2(rect.left() - 52.0, rect.top() + 10.0),
            egui::Align2::LEFT_CENTER,
            if repeats_mode { "Repeats" } else { "Velocity" },
            egui::FontId::proportional(9.0),
            egui::Color32::from_gray(150),
        );
        let instrument = self.selected_instrument;
        let visible_end = absolute_start.saturating_add(VISIBLE_PIANO_STEPS);
        let notes: Vec<(usize, f32, u8)> = self.project.patterns[self.selected_pattern]
            .notes
            .iter()
            .enumerate()
            .filter(|(_, note)| {
                note.instrument_id == instrument
                    && note.start_step < visible_end
                    && note.start_step.saturating_add(note.duration_steps) > absolute_start
            })
            .map(|(index, note)| {
                let value = if repeats_mode {
                    // Whole repeats map onto the lane's height, so the bars snap
                    // to integer counts instead of a 0..127 velocity scale.
                    ((u32::from(note.repeat_count) * 127) / 32).min(127) as u8
                } else {
                    note.velocity.clamp(1, 127)
                };
                (index, note.start_tick() as f32 / 16.0, value)
            })
            .collect();
        for (_, start, velocity) in &notes {
            let x = rect.left() + (start - absolute_start as f32) * step_width;
            let height = (f32::from(*velocity) / 127.0).clamp(0.0, 1.0) * (rect.height() - 6.0);
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x + 1.0, rect.bottom() - height),
                    egui::pos2((x + step_width - 1.0).max(x + 3.0), rect.bottom()),
                ),
                1.0,
                lane_color(instrument),
            );
        }
        let (pressed, down, released) = context.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_down(egui::PointerButton::Primary),
                input.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        let dragged = response.drag_stopped();
        if pressed && response.hovered() {
            self.push_history();
            self.piano_gesture = None;
        }
        if (down && (response.hovered() || response.dragged()))
            && let Some(pointer) = context.input(|input| input.pointer.interact_pos())
            && rect.expand2(egui::vec2(0.0, 40.0)).contains(pointer)
        {
            let position =
                ((pointer.x - rect.left()) / step_width + absolute_start as f32).max(0.0);
            let velocity =
                (((rect.bottom() - pointer.y) / rect.height()) * 127.0).clamp(1.0, 127.0) as u8;
            let target = notes
                .iter()
                .min_by(|left, right| {
                    (left.1 - position)
                        .abs()
                        .total_cmp(&(right.1 - position).abs())
                })
                .map(|(index, start, _)| (*index, *start));
            if let Some((index, start)) = target
                && (start - position).abs() <= 1.0
            {
                let note = &mut self.project.patterns[self.selected_pattern].notes[index];
                if repeats_mode {
                    note.repeat_count =
                        ((f32::from(velocity) / 127.0 * 32.0).round() as u8).min(32);
                } else {
                    note.velocity = velocity;
                }
                self.sync_project();
            }
        }
        if released || dragged {
            self.piano_gesture = None;
        }
    }

    fn handle_piano_input(
        &mut self,
        grid_rect: egui::Rect,
        response: egui::Response,
        context: &egui::Context,
        step_width: f32,
        row_height: f32,
    ) {
        let (
            pointer,
            snap,
            alt,
            secondary_down,
            secondary_pressed,
            middle_down,
            primary_pressed,
            primary_down,
            primary_released,
        ) = context.input(|input| {
            (
                input.pointer.interact_pos(),
                self.project.snap_ticks.max(1),
                input.modifiers.alt,
                input.pointer.button_down(egui::PointerButton::Secondary),
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.pointer.button_down(egui::PointerButton::Middle),
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_down(egui::PointerButton::Primary),
                input.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        let Some(pointer) = pointer else {
            if primary_released || response.drag_stopped() {
                self.finish_piano_gesture();
            }
            return;
        };
        // Right-click starts delete mode; dropping the selection first keeps the
        // following actions local to what is under the pointer.
        if secondary_pressed {
            self.clear_selection();
        }
        let inside_grid = grid_rect.contains(pointer)
            && (response.contains_pointer() || self.piano_gesture.is_some());
        if !inside_grid && !(primary_down && self.piano_gesture.is_some()) {
            if primary_released || response.drag_stopped() {
                self.finish_piano_gesture();
            }
            return;
        }
        let pointer = egui::pos2(
            pointer.x.clamp(grid_rect.left(), grid_rect.right() - 0.01),
            pointer.y.clamp(grid_rect.top(), grid_rect.bottom() - 0.01),
        );
        let raw_tick = self.piano_pan_steps.saturating_mul(16)
            + (((pointer.x - grid_rect.left()) / step_width) * 16.0)
                .floor()
                .max(0.0) as u64;
        let tick = piano_snap_tick(raw_tick, snap, alt);
        let row = ((pointer.y - grid_rect.top()) / row_height)
            .floor()
            .max(0.0) as usize;
        if row >= PIANO_ROWS {
            return;
        }
        let raw_pitch = PIANO_MIN_PITCH + (PIANO_ROWS - 1 - row) as u8;
        // Note snapping pulls drawn and moved notes onto the selected scale.
        let pitch = self.snap_pitch_to_scale(raw_pitch);
        if middle_down {
            if !inside_grid {
                return;
            }
            let delta = context.input(|input| input.pointer.delta().x);
            self.piano_pan_steps = self
                .piano_pan_steps
                .saturating_add_signed((-delta / step_width).round() as i64);
            return;
        }
        if secondary_down {
            self.push_history();
            self.remove_note_at(tick, raw_pitch);
            return;
        }
        // Ctrl+drag rubber-band selection: the Select tool has no gesture of its
        // own, so the band is tracked separately from `piano_gesture`.
        if self.piano_box_origin.is_some() {
            if primary_down {
                if let Some((from_tick, from_pitch)) = self.piano_box_origin {
                    self.select_notes_in_box(from_tick, tick, from_pitch, pitch);
                }
            } else {
                self.piano_box_origin = None;
            }
        }

        if primary_pressed {
            // `pitch` is snapped to the scale for notes we create; hit tests must
            // use the row the pointer is actually on, or notes outside the scale
            // can never be selected, moved or deleted.
            self.start_piano_gesture(tick, pitch, raw_pitch, pointer, grid_rect, step_width);
        }
        if primary_down {
            self.update_piano_gesture(tick, pitch, pointer);
        }
        if primary_released || response.drag_stopped_by(egui::PointerButton::Primary) {
            self.finish_piano_gesture();
        }
    }

    fn paint_tool_cursor(
        &self,
        ui: &mut egui::Ui,
        context: &egui::Context,
        grid_rect: egui::Rect,
        step_width: f32,
        row_height: f32,
    ) {
        let Some(pointer) = context
            .input(|input| input.pointer.interact_pos())
            .filter(|position| grid_rect.contains(*position))
        else {
            return;
        };
        let (middle_down, secondary_down) = context.input(|input| {
            (
                input.pointer.button_down(egui::PointerButton::Middle),
                input.pointer.button_down(egui::PointerButton::Secondary),
            )
        });
        let mut cursor_kind = match self.piano_tool {
            PianoTool::Pen => "pen",
            PianoTool::Select => "move",
            PianoTool::Brush => "brush",
            PianoTool::Join => "join",
            PianoTool::Cut => "cut",
        };
        if middle_down {
            cursor_kind = "pan";
        } else {
            let row = ((pointer.y - grid_rect.top()) / row_height)
                .floor()
                .max(0.0) as usize;
            let pitch = PIANO_MIN_PITCH + (PIANO_ROWS - 1 - row.min(PIANO_ROWS - 1)) as u8;
            let hovered_note = self.project.patterns[self.selected_pattern]
                .notes
                .iter()
                .find(|note| {
                    if note.instrument_id != self.selected_instrument || note.pitch != pitch {
                        return false;
                    }
                    let start_x = grid_rect.left()
                        + (note.start_tick() as f32 / 16.0 - self.piano_pan_steps as f32)
                            * step_width;
                    let end_x = grid_rect.left()
                        + ((note.start_tick().saturating_add(note.duration_ticks())) as f32 / 16.0
                            - self.piano_pan_steps as f32)
                            * step_width;
                    pointer.x >= start_x.max(grid_rect.left())
                        && pointer.x <= end_x.min(grid_rect.right())
                });
            if let Some(note) = hovered_note {
                let note_start_x = grid_rect.left()
                    + (note.start_tick() as f32 / 16.0 - self.piano_pan_steps as f32) * step_width;
                let note_end_x = grid_rect.left()
                    + ((note.start_tick().saturating_add(note.duration_ticks())) as f32 / 16.0
                        - self.piano_pan_steps as f32)
                        * step_width;
                cursor_kind = if secondary_down {
                    "delete"
                } else if self.piano_tool == PianoTool::Pen
                    && (pointer.x >= note_end_x - 8.0
                        || (note.start_tick() >= self.piano_pan_steps.saturating_mul(16)
                            && pointer.x <= note_start_x + 8.0))
                {
                    "resize"
                } else {
                    "move"
                };
            } else if secondary_down {
                cursor_kind = "delete";
            }
        }

        let native_icon = match cursor_kind {
            "resize" => egui::CursorIcon::ResizeHorizontal,
            "move" | "join" => egui::CursorIcon::PointingHand,
            "pan" => egui::CursorIcon::Grabbing,
            "delete" => egui::CursorIcon::NoDrop,
            _ => egui::CursorIcon::Crosshair,
        };
        context.set_cursor_icon(native_icon);
        // The cut tool previews its split point on the exact tick it will use.
        if self.piano_tool == PianoTool::Cut && !middle_down {
            let alt = context.input(|input| input.modifiers.alt);
            let raw_tick = self.piano_pan_steps.saturating_mul(16)
                + (((pointer.x - grid_rect.left()) / step_width) * 16.0)
                    .floor()
                    .max(0.0) as u64;
            let tick = piano_snap_tick(raw_tick, self.project.snap_ticks, alt);
            let cut_x = grid_rect.left()
                + (tick as f32 / 16.0 - self.piano_pan_steps as f32) * step_width;
            if cut_x >= grid_rect.left() && cut_x <= grid_rect.right() {
                let stroke = egui::Stroke::new(1.4_f32, egui::Color32::from_rgb(232, 96, 88));
                let painter = ui.painter();
                let mut y = grid_rect.top();
                while y < grid_rect.bottom() {
                    painter.line_segment(
                        [egui::pos2(cut_x, y), egui::pos2(cut_x, (y + 5.0).min(grid_rect.bottom()))],
                        stroke,
                    );
                    y += 10.0;
                }
            }
        }
        paint_svg_cursor(ui, cursor_kind, pointer);
    }

    fn start_piano_gesture(
        &mut self,
        tick: u64,
        pitch: u8,
        hit_pitch: u8,
        pointer: egui::Pos2,
        grid_rect: egui::Rect,
        step_width: f32,
    ) {
        let step = tick / 16;
        self.push_history();
        let pattern = &mut self.project.patterns[self.selected_pattern];
        match self.piano_tool {
            PianoTool::Pen => {
                if let Some((index, note)) = pattern.notes.iter().enumerate().find(|(_, note)| {
                    note.instrument_id == self.selected_instrument
                        && note.pitch == hit_pitch
                        && tick >= note.start_tick()
                        && tick < note.start_tick().saturating_add(note.duration_ticks())
                }) {
                    self.selection = Some(Selection::Note {
                        pattern: self.selected_pattern,
                        instrument: note.instrument_id,
                        pitch: note.pitch,
                        start_step: note.start_step,
                    });
                    let start_tick = note.start_tick();
                    let end_tick = start_tick.saturating_add(note.duration_ticks());
                    let end_x = grid_rect.left()
                        + (end_tick as f32 / 16.0 - self.piano_pan_steps as f32) * step_width;
                    let start_x = grid_rect.left()
                        + (start_tick as f32 / 16.0 - self.piano_pan_steps as f32) * step_width;
                    // 7 px edge zones: left resizes the start, right the end.
                    const EDGE: f32 = 7.0;
                    if (pointer.x - end_x).abs() <= EDGE {
                        self.piano_gesture = Some(PianoGesture::Resize {
                            note_index: index,
                            origin_duration_ticks: note.duration_ticks(),
                            pointer_tick: tick,
                        });
                    } else if (pointer.x - start_x).abs() <= EDGE
                        && start_tick >= self.piano_pan_steps.saturating_mul(16)
                    {
                        self.piano_gesture = Some(PianoGesture::ResizeStart {
                            note_index: index,
                            origin_start_tick: start_tick,
                            origin_duration_ticks: note.duration_ticks(),
                            pointer_tick: tick,
                        });
                    } else {
                        self.piano_gesture = Some(PianoGesture::Move {
                            note_index: index,
                            origin_tick: start_tick,
                            origin_pitch: note.pitch,
                            pointer_tick: tick,
                            pointer_pitch: pitch,
                        });
                    }
                } else {
                    let note_index = pattern.notes.len();
                    let mut note = PianoNote {
                        instrument_id: self.selected_instrument,
                        duration_steps: 1,
                        pitch,
                        velocity: self.settings.default_velocity,
                        repeat_count: self.note_repeat_count,
                        repeat_ticks: self.note_repeat_ticks,
                        repeat_decay: self.note_repeat_decay,
                        ..PianoNote::default()
                    };
                    note.set_start_tick(tick);
                    note.set_duration_ticks(self.note_length_steps.max(1).saturating_mul(16));
                    pattern.notes.push(note);
                    self.selection = Some(Selection::Note {
                        pattern: self.selected_pattern,
                        instrument: self.selected_instrument,
                        pitch,
                        start_step: tick / 16,
                    });
                    self.piano_gesture = Some(PianoGesture::Draw {
                        pitch,
                        start_tick: tick,
                        note_index,
                        default_duration_ticks: self.note_length_steps.max(1).saturating_mul(16),
                    });
                }
            }
            PianoTool::Brush => {
                // The brush paints notes at the hovered pitch, like FL's paint
                // tool, instead of toggling step-sequencer rows.
                let exists = pattern.notes.iter().position(|note| {
                    note.instrument_id == self.selected_instrument
                        && note.pitch == pitch
                        && note.start_step == step
                });
                let note_index = match exists {
                    Some(index) => index,
                    None => {
                        let mut note = PianoNote {
                            instrument_id: self.selected_instrument,
                            pitch,
                            duration_steps: 1,
                            velocity: self.settings.default_velocity,
                            repeat_count: self.note_repeat_count,
                            repeat_ticks: self.note_repeat_ticks,
                            repeat_decay: self.note_repeat_decay,
                            ..PianoNote::default()
                        };
                        note.set_start_tick(tick);
                        pattern.notes.push(note);
                        pattern.notes.len() - 1
                    }
                };
                self.piano_gesture = Some(PianoGesture::Draw {
                    pitch,
                    start_tick: tick,
                    note_index,
                    default_duration_ticks: 16,
                });
            }
            PianoTool::Select => {
                // Clicking a note selects it, clicking empty space clears the
                // selection; shift and ctrl extend it.
                let hit = {
                    let notes = &self.project.patterns[self.selected_pattern].notes;
                    notes.iter().position(|note| {
                        note.instrument_id == self.selected_instrument
                            && note.pitch == hit_pitch
                            && tick >= note.start_tick()
                            && tick < note.start_tick().saturating_add(note.duration_ticks())
                    })
                };
                let additive = self.selection_modifiers();
                match hit {
                    Some(index) => {
                        let note = self.project.patterns[self.selected_pattern].notes[index];
                        self.select_note(
                            index,
                            note.instrument_id,
                            note.pitch,
                            note.start_step,
                            additive.0,
                            additive.1,
                        );
                    }
                    None => {
                        if !additive.0 && !additive.1 {
                            self.clear_selection();
                        }
                    }
                }
                // Ctrl+drag always starts a rubber band, even when the press
                // lands on a note.
                if additive.1 {
                    self.piano_box_origin = Some((tick, pitch));
                }
            }
            PianoTool::Join => {
                self.piano_gesture = Some(PianoGesture::Join {
                    pitch,
                    start_step: tick,
                    end_step: tick,
                });
            }
            PianoTool::Cut => self.cut_note_at(tick, pitch),
        }
        let _ = (pointer, grid_rect);
    }

    fn update_piano_gesture(&mut self, tick: u64, pitch: u8, _pointer: egui::Pos2) {
        let step = tick / 16;
        match self.piano_gesture {
            Some(PianoGesture::Draw {
                pitch: draw_pitch,
                start_tick,
                note_index,
                default_duration_ticks,
            }) => {
                if let Some(note) = self.project.patterns[self.selected_pattern]
                    .notes
                    .get_mut(note_index)
                {
                    let end_tick = tick.max(start_tick);
                    note.set_duration_ticks(if end_tick == start_tick {
                        default_duration_ticks
                    } else {
                        end_tick.saturating_sub(start_tick).max(1)
                    });
                    note.pitch = draw_pitch;
                }
            }
            Some(PianoGesture::Move {
                note_index,
                origin_tick,
                origin_pitch,
                pointer_tick,
                pointer_pitch,
            }) => {
                let delta_tick = tick as i128 - pointer_tick as i128;
                let delta_pitch = i16::from(pitch) - i16::from(pointer_pitch);
                if let Some(note) = self.project.patterns[self.selected_pattern]
                    .notes
                    .get_mut(note_index)
                {
                    note.set_start_tick((origin_tick as i128 + delta_tick).max(0) as u64);
                    note.pitch = (i16::from(origin_pitch) + delta_pitch).clamp(0, 127) as u8;
                }
            }
            Some(PianoGesture::Resize {
                note_index,
                origin_duration_ticks,
                pointer_tick,
            }) => {
                if let Some(note) = self.project.patterns[self.selected_pattern]
                    .notes
                    .get_mut(note_index)
                {
                    let delta = tick as i128 - pointer_tick as i128;
                    note.set_duration_ticks(
                        (origin_duration_ticks as i128 + delta).clamp(1, u64::MAX as i128) as u64,
                    );
                }
            }
            Some(PianoGesture::ResizeStart {
                note_index,
                origin_start_tick,
                origin_duration_ticks,
                pointer_tick,
            }) => {
                if let Some(note) = self.project.patterns[self.selected_pattern]
                    .notes
                    .get_mut(note_index)
                {
                    let (start_tick, duration_ticks) = resize_note_start(
                        origin_start_tick,
                        origin_duration_ticks,
                        pointer_tick,
                        tick,
                    );
                    note.set_start_tick(start_tick);
                    note.set_duration_ticks(duration_ticks);
                }
            }
            Some(PianoGesture::Join {
                pitch: join_pitch,
                start_step,
                ..
            }) => {
                self.piano_gesture = Some(PianoGesture::Join {
                    pitch: join_pitch,
                    start_step,
                    end_step: step,
                });
            }
            _ => {}
        }
        let note_index = match self.piano_gesture {
            Some(PianoGesture::Draw { note_index, .. })
            | Some(PianoGesture::Move { note_index, .. })
            | Some(PianoGesture::Resize { note_index, .. })
            | Some(PianoGesture::ResizeStart { note_index, .. }) => Some(note_index),
            _ => None,
        };
        if let Some(note) = note_index.and_then(|index| {
            self.project.patterns[self.selected_pattern]
                .notes
                .get(index)
                .copied()
        }) {
            self.selection = Some(Selection::Note {
                pattern: self.selected_pattern,
                instrument: note.instrument_id,
                pitch: note.pitch,
                start_step: note.start_step,
            });
        }
    }

    fn finish_piano_gesture(&mut self) {
        if let Some(PianoGesture::Join {
            pitch,
            start_step,
            end_step,
        }) = self.piano_gesture
        {
            self.join_notes(pitch, start_step.min(end_step), start_step.max(end_step));
        }
        if self.piano_gesture.is_some() {
            self.project.patterns[self.selected_pattern]
                .notes
                .sort_by_key(|note| (note.start_step, note.pitch, note.instrument_id));
            self.sync_project();
        }
        self.piano_gesture = None;
    }

    fn remove_note_at(&mut self, tick: u64, pitch: u8) {
        let notes = &mut self.project.patterns[self.selected_pattern].notes;
        let selected_note_was_removed = notes.iter().any(|note| {
            note.instrument_id == self.selected_instrument
                && note.pitch == pitch
                && tick >= note.start_tick()
                && tick < note.start_tick().saturating_add(note.duration_ticks())
                && self.selection
                    == Some(Selection::Note {
                        pattern: self.selected_pattern,
                        instrument: note.instrument_id,
                        pitch: note.pitch,
                        start_step: note.start_step,
                    })
        });
        let before = notes.len();
        notes.retain(|note| {
            !(note.instrument_id == self.selected_instrument
                && note.pitch == pitch
                && tick >= note.start_tick()
                && tick < note.start_tick().saturating_add(note.duration_ticks()))
        });
        if notes.len() != before {
            if selected_note_was_removed {
                self.selection = None;
            }
            self.sync_project();
        }
    }

    fn cut_note_at(&mut self, tick: u64, pitch: u8) {
        let notes = &mut self.project.patterns[self.selected_pattern].notes;
        let Some(index) = notes.iter().position(|note| {
            note.instrument_id == self.selected_instrument
                && note.pitch == pitch
                && tick > note.start_tick()
                && tick < note.start_tick().saturating_add(note.duration_ticks())
        }) else {
            return;
        };
        let original = notes[index];
        let first_duration = tick - original.start_tick();
        let second_duration = original.duration_ticks() - first_duration;
        notes[index].set_duration_ticks(first_duration);
        let mut second = original;
        second.set_start_tick(tick);
        second.set_duration_ticks(second_duration);
        notes.push(second);
        self.sync_project();
    }

    fn join_notes(&mut self, pitch: u8, start: u64, end: u64) {
        let notes = &mut self.project.patterns[self.selected_pattern].notes;
        let mut matching: Vec<_> = notes
            .iter()
            .copied()
            .filter(|note| {
                note.instrument_id == self.selected_instrument
                    && note.pitch == pitch
                    && note.start_tick() <= end
                    && note.start_tick().saturating_add(note.duration_ticks()) >= start
            })
            .collect();
        if matching.len() < 2 {
            return;
        }
        matching.sort_by_key(|note| note.start_tick());
        let merged_start = matching.first().unwrap().start_tick();
        let merged_end = matching
            .iter()
            .map(|note| note.start_tick().saturating_add(note.duration_ticks()))
            .max()
            .unwrap_or(merged_start + 16);
        let template = matching[0];
        notes.retain(|note| {
            !(note.instrument_id == self.selected_instrument
                && note.pitch == pitch
                && note.start_tick() <= end
                && note.start_tick().saturating_add(note.duration_ticks()) >= start)
        });
        let mut merged = template;
        merged.set_start_tick(merged_start);
        merged.set_duration_ticks(merged_end.saturating_sub(merged_start).max(1));
        notes.push(merged);
        self.selection = Some(Selection::Note {
            pattern: self.selected_pattern,
            instrument: template.instrument_id,
            pitch: template.pitch,
            start_step: merged_start / 16,
        });
    }

    /// Shared plug-in window header. Returns true when the settings tab is
    /// active, which means the window shows routing/voice options instead of
    /// the plug-in interface.
    fn plugin_window_header(
        &mut self,
        ui: &mut egui::Ui,
        target: PluginTarget,
        title: &str,
    ) -> bool {
        let settings_open = self.plugin_settings_tab.contains(&target);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.selectable_label(settings_open, "Settings").clicked() {
                    if settings_open {
                        self.plugin_settings_tab.remove(&target);
                    } else {
                        self.plugin_settings_tab.insert(target);
                    }
                }
                if ui.selectable_label(!settings_open, "Plug-in").clicked() {
                    self.plugin_settings_tab.remove(&target);
                }
            });
        });
        ui.separator();
        self.plugin_settings_tab.contains(&target)
    }

    /// Plug-in settings tab: routing, voice count and the arpeggiator.
    ///
    /// The two groups sit side by side so the page stays short instead of
    /// turning into a long vertical strip.
    fn render_plugin_routing(&mut self, ui: &mut egui::Ui, instrument_id: usize) {
        let Some(instrument) = self.project.instruments.get_mut(instrument_id) else {
            return;
        };
        ui.columns(2, |columns| {
            let left = &mut columns[0];
            left.label(egui::RichText::new("Routing").strong());
            egui::Grid::new(("plugin_routing", instrument_id))
                .num_columns(2)
                .show(left, |ui| {
                    ui.label("MIDI in channel");
                    ui.add(
                        egui::DragValue::new(&mut instrument.voices.midi_in_channel).range(0..=15),
                    );
                    ui.end_row();
                    ui.label("MIDI out channel");
                    ui.add(
                        egui::DragValue::new(&mut instrument.voices.midi_out_channel).range(0..=15),
                    );
                    ui.end_row();
                    ui.label("Audio in");
                    ui.add(egui::DragValue::new(&mut instrument.voices.audio_in).range(0..=15));
                    ui.end_row();
                    ui.label("Audio out");
                    ui.add(egui::DragValue::new(&mut instrument.voices.audio_out).range(0..=15));
                    ui.end_row();
                    ui.label("Voices");
                    ui.add(egui::DragValue::new(&mut instrument.voices.voice_count).range(1..=64));
                    ui.end_row();
                });

            let right = &mut columns[1];
            right.label(egui::RichText::new("Arpeggiator").strong());
            right.checkbox(&mut instrument.arpeggio.enabled, "Enabled");
            egui::Grid::new(("plugin_arp", instrument_id))
                .num_columns(2)
                .show(right, |ui| {
                    ui.label("Mode");
                    egui::ComboBox::from_id_salt(("arp_mode", instrument_id))
                        .selected_text(instrument.arpeggio.mode.label())
                        .show_ui(ui, |ui| {
                            for mode in project::ArpMode::ALL {
                                ui.selectable_value(
                                    &mut instrument.arpeggio.mode,
                                    mode,
                                    mode.label(),
                                );
                            }
                        });
                    ui.end_row();
                    ui.label("Gate");
                    ui.add(egui::Slider::new(&mut instrument.arpeggio.gate, 0.05..=1.0));
                    ui.end_row();
                    ui.label("Octaves");
                    ui.add(egui::Slider::new(&mut instrument.arpeggio.octaves, 1..=4));
                    ui.end_row();
                });
            edit_time_value(right, instrument_id, "Rate", &mut instrument.arpeggio.rate);
        });
    }

    fn render_instrument_plugin(&mut self, instrument_id: usize, ui: &mut egui::Ui) {
        let name = self
            .project
            .instruments
            .get(instrument_id)
            .map_or("Plug-in".to_owned(), |instrument| instrument.name.clone());
        if self.plugin_window_header(ui, PluginTarget::Instrument(instrument_id), &name) {
            self.render_plugin_routing(ui, instrument_id);
            if ui.input(|input| input.pointer.any_released()) {
                self.sync_project();
            }
            return;
        }
        let show_waveform = self
            .project
            .instruments
            .get(instrument_id)
            .is_some_and(|instrument| instrument.synth_kind == SynthKind::SamplePlayer);
        let waveform_rect = show_waveform.then(|| {
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover())
                .0
        });
        if let Some(rect) = waveform_rect {
            self.draw_sample_waveform(ui.painter_at(rect), rect, instrument_id);
        }
        let Some(instrument) = self.project.instruments.get_mut(instrument_id) else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Instrument no longer exists.");
            return;
        };
        let mut changed = false;
        let mut load_sample = false;
        ui.horizontal(|ui| {
            ui.heading(&instrument.name);
            ui.label(format!("· {}", synth_kind_label(instrument.synth_kind)));
            if ui.button("Select plugin...").clicked() {
                self.plugin_selector_target = Some(PluginTarget::Instrument(instrument_id));
            }
        });
        ui.separator();

        match instrument.synth_kind {
            SynthKind::External => {
                self.render_external_plugin_wrapper(ui, instrument_id);
            }
            SynthKind::SamplePlayer => {
                ui.heading("Sample Player");
                ui.label("Plays the instrument's sample on sequencer and MIDI triggers.");
                if waveform_rect.is_some() {
                    ui.separator();
                }
                let mut path = instrument.sample_path.clone();
                ui.horizontal(|ui| {
                    ui.label("WAV");
                    ui.add(
                        egui::TextEdit::singleline(&mut path)
                            .desired_width(330.0)
                            .hint_text("Path to a WAV sample"),
                    );
                    if ui.button("Load sample").clicked() {
                        instrument.sample_path = path.clone();
                        load_sample = true;
                        changed = true;
                    }
                });
                if path != instrument.sample_path {
                    instrument.sample_path = path;
                    changed = true;
                }
                ui.horizontal(|ui| {
                    ui.label("Default sound");
                    egui::ComboBox::from_id_salt(("default_sample", instrument_id))
                        .selected_text(default_sample_label(instrument.default_sample))
                        .show_ui(ui, |ui| {
                            for (sample, label) in [
                                (DefaultSample::None, "None"),
                                (DefaultSample::Kick, "Kick"),
                                (DefaultSample::Snare, "Snare"),
                                (DefaultSample::HiHat, "Hi-hat"),
                            ] {
                                changed |= ui
                                    .selectable_value(&mut instrument.default_sample, sample, label)
                                    .changed();
                            }
                        });
                });
                ui.horizontal(|ui| {
                    changed |= Knob::new("LEVEL", 0.0, 1.0, "")
                        .with_default(0.8)
                        .automates(ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::Gain,
                        })
                        .show(ui, &mut instrument.gain);
                    changed |= Knob::new("PAN", -1.0, 1.0, "")
                        .with_default(0.0)
                        .automates(ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::Pan,
                        })
                        .show(ui, &mut instrument.pan);
                });
            }
            kind => {
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut instrument.synth_enabled, "Enabled")
                        .changed();
                    changed |= Knob::new("LEVEL", 0.0, 1.0, "")
                        .with_default(0.8)
                        .automates(ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::Gain,
                        })
                        .show(ui, &mut instrument.gain);
                    changed |= Knob::new("PAN", -1.0, 1.0, "")
                        .with_default(0.0)
                        .automates(ParameterTarget::Instrument {
                            instrument_id,
                            parameter: InstrumentParameter::Pan,
                        })
                        .show(ui, &mut instrument.pan);
                });
                match kind {
                    // Handled by the wrapper above; kept for exhaustiveness.
                    SynthKind::External => {}
                    SynthKind::SignalGenerator => {
                        changed |= ui
                            .add(
                                egui::Slider::new(&mut instrument.synth_note, 0..=127)
                                    .text("MIDI note"),
                            )
                            .changed();
                        egui::ComboBox::from_id_salt(("signal_waveform_editor", instrument_id))
                            .selected_text(waveform_label(instrument.signal_waveform))
                            .show_ui(ui, |ui| {
                                for (waveform, label) in [
                                    (SignalWaveform::Sine, "Sine"),
                                    (SignalWaveform::Triangle, "Triangle"),
                                    (SignalWaveform::Saw, "Saw"),
                                    (SignalWaveform::Square, "Square"),
                                ] {
                                    changed |= ui
                                        .selectable_value(
                                            &mut instrument.signal_waveform,
                                            waveform,
                                            label,
                                        )
                                        .changed();
                                }
                            });
                    }
                    SynthKind::Ms20 => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("CUTOFF", 40.0, 16_000.0, " Hz")
                                .with_default(1_800.0)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::CutoffHz,
                                })
                                .show(ui, &mut instrument.ms20_cutoff_hz);
                            changed |= Knob::new("RESONANCE", 0.0, 0.9, "")
                                .with_default(0.2)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::Resonance,
                                })
                                .show(ui, &mut instrument.ms20_resonance);
                        });
                    }
                    SynthKind::JunoPoly => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("CUTOFF", 40.0, 16_000.0, " Hz")
                                .with_default(2_400.0)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::CutoffHz,
                                })
                                .show(ui, &mut instrument.juno_cutoff_hz);
                            changed |= Knob::new("RESONANCE", 0.0, 0.9, "")
                                .with_default(0.15)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::Resonance,
                                })
                                .show(ui, &mut instrument.juno_resonance);
                            changed |= Knob::new("PULSE", 0.05, 0.95, "")
                                .with_default(0.5)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::PulseWidth,
                                })
                                .show(ui, &mut instrument.juno_pulse_width);
                            changed |= Knob::new("SUB", 0.0, 1.0, "")
                                .with_default(0.3)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::SubLevel,
                                })
                                .show(ui, &mut instrument.juno_sub_level);
                        });
                    }
                    SynthKind::M1Workstation => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("TONE", 0.0, 1.0, "")
                                .with_default(0.55)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::Tone,
                                })
                                .show(ui, &mut instrument.m1_tone);
                            egui::ComboBox::from_id_salt(("m1_decay_unit", instrument_id))
                                .selected_text(match instrument.m1_decay_unit {
                                    project::TimeUnit::Steps => "Steps",
                                    project::TimeUnit::Milliseconds => "ms",
                                })
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut instrument.m1_decay_unit,
                                            project::TimeUnit::Steps,
                                            "Steps",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut instrument.m1_decay_unit,
                                            project::TimeUnit::Milliseconds,
                                            "Milliseconds",
                                        )
                                        .changed();
                                });
                            match instrument.m1_decay_unit {
                                project::TimeUnit::Steps => {
                                    changed |= Knob::new("DECAY", 0.01, 256.0, " steps")
                                        .with_default(4.0)
                                        .automates(ParameterTarget::Instrument {
                                            instrument_id,
                                            parameter: InstrumentParameter::Decay,
                                        })
                                        .show(ui, &mut instrument.m1_decay_steps);
                                }
                                project::TimeUnit::Milliseconds => {
                                    let mut decay_ms = instrument.m1_decay * 1_000.0;
                                    let decay_changed = Knob::new("DECAY", 50.0, 4_000.0, " ms")
                                        .with_default(1_200.0)
                                        .automates(ParameterTarget::Instrument {
                                            instrument_id,
                                            parameter: InstrumentParameter::Decay,
                                        })
                                        .show(ui, &mut decay_ms);
                                    if decay_changed {
                                        instrument.m1_decay = decay_ms / 1_000.0;
                                        changed = true;
                                    }
                                }
                            }
                        });
                    }
                    SynthKind::FmSynth => {
                        ui.horizontal(|ui| {
                            changed |= Knob::new("RATIO", 0.25, 16.0, "")
                                .with_default(2.0)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::FmRatio,
                                })
                                .show(ui, &mut instrument.fm_ratio);
                            changed |= Knob::new("FM INDEX", 0.0, 1.0, "")
                                .with_default(0.35)
                                .automates(ParameterTarget::Instrument {
                                    instrument_id,
                                    parameter: InstrumentParameter::FmIndex,
                                })
                                .show(ui, &mut instrument.fm_index);
                        });
                    }
                    SynthKind::Sine | SynthKind::SamplePlayer => {}
                }
            }
        }
        if changed {
            self.sync_project();
        }
        if load_sample {
            self.load_instrument_sample(instrument_id);
        }
    }

    fn render_effect_plugin(&mut self, channel: usize, slot_id: usize, ui: &mut egui::Ui) {
        let title = self
            .effect_slot_name(channel, slot_id)
            .unwrap_or_else(|| "Empty insert".to_owned());
        if self.plugin_window_header(
            ui,
            PluginTarget::Effect {
                channel,
                slot: slot_id,
            },
            &title,
        ) {
            if let Some(effect) = self.effect_slot_mut(channel, slot_id) {
                ui.checkbox(&mut effect.enabled, "Enabled");
                ui.checkbox(&mut effect.locked, "Locked");
                ui.add(egui::Slider::new(&mut effect.mix, 0.0..=1.0).text("Mix"));
                ui.label(
                    "Routing and latency settings are not implemented for insert effects yet.",
                );
            }
            if ui.input(|input| input.pointer.any_released()) {
                self.sync_project();
            }
            return;
        }
        let lane_count = self.project.lanes.len();
        let channel_label = if channel == lane_count {
            "MASTER".to_owned()
        } else if let Some(lane) = self.project.lanes.get(channel) {
            lane.name.clone()
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Mixer channel no longer exists.");
            return;
        };
        let slots = if channel == lane_count {
            &mut self.project.master.inserts
        } else if let Some(lane) = self.project.lanes.get_mut(channel) {
            &mut lane.inserts
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Mixer channel no longer exists.");
            return;
        };
        let Some(slot) = slots.get_mut(slot_id) else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Insert slot no longer exists.");
            return;
        };
        ui.horizontal(|ui| {
            ui.heading(format!("{} · {}", channel_label, slot.name));
            if ui.button("Select effect...").clicked() {
                self.plugin_selector_target = Some(PluginTarget::Effect {
                    channel,
                    slot: slot_id,
                });
            }
        });
        let mut changed = ui.checkbox(&mut slot.enabled, "Enabled").changed();
        changed |= render_effect_parameters(ui, slot, channel, slot_id);
        let plugin_path = slot.plugin_path.clone();
        let hosted = slot.enabled && !plugin_path.is_empty();
        if changed {
            self.sync_project();
        }
        if hosted {
            let host_id = pulseforge_core::hosted::effect_host_id(channel, slot_id);
            if self.hosted.contains_key(&host_id) {
                self.render_hosted_plugin(ui, host_id);
            } else {
                ui.horizontal(|ui| {
                    ui.label("Hosted plug-in is not loaded.");
                    if ui.button("Load plug-in").clicked() {
                        self.load_host(host_id, &plugin_path, false);
                    }
                });
                if let Some(error) = self.hosted_errors.get(&host_id) {
                    ui.colored_label(egui::Color32::from_rgb(230, 140, 110), error);
                }
            }
        }
    }

    fn render_plugin_selector(&mut self, ui: &mut egui::Ui, target: PluginTarget) {
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(
                egui::TextEdit::singleline(&mut self.plugin_search)
                    .desired_width(200.0)
                    .hint_text("Plug-in name"),
            );
            if ui.small_button("Clear").clicked() {
                self.plugin_search.clear();
            }
        });
        let filter = self.plugin_search.to_ascii_lowercase();
        let matches = |name: &str| filter.is_empty() || name.to_ascii_lowercase().contains(&filter);
        ui.separator();
        match target {
            PluginTarget::Instrument(instrument_id) => {
                let Some(instrument) = self.project.instruments.get(instrument_id) else {
                    ui.colored_label(egui::Color32::LIGHT_RED, "Instrument no longer exists.");
                    return;
                };
                ui.label(format!("Choose a plug-in for {}", instrument.name));
                let current = instrument.synth_kind;
                for kind in SynthKind::BUILTIN {
                    if !matches(synth_kind_label(kind)) {
                        continue;
                    }
                    if ui
                        .selectable_label(current == kind, synth_kind_label(kind))
                        .clicked()
                    {
                        // Choosing a built-in must clear any hosted plug-in so
                        // the wrapper window cannot claim the slot.
                        let host_id = pulseforge_core::hosted::instrument_host_id(instrument_id);
                        self.unload_host(host_id);
                        let instrument = &mut self.project.instruments[instrument_id];
                        instrument.synth_kind = kind;
                        instrument.plugin_path.clear();
                        instrument.enabled = true;
                        instrument.synth_enabled = kind != SynthKind::SamplePlayer;
                        self.open_instrument_plugins.insert(instrument_id);
                        self.plugin_selector_target = None;
                        self.sync_project();
                        break;
                    }
                }
                ui.separator();
                ui.label(egui::RichText::new("Installed plug-ins").strong());
                for plugin in self.external_plugins.clone() {
                    if !matches(&plugin.label()) {
                        continue;
                    }
                    let active = self
                        .project
                        .instruments
                        .get(instrument_id)
                        .is_some_and(|instrument| instrument.plugin_path == plugin.path);
                    if ui
                        .selectable_label(active, plugin.label())
                        .on_hover_text(&plugin.path)
                        .clicked()
                    {
                        let host_id = pulseforge_core::hosted::instrument_host_id(instrument_id);
                        let path = plugin.path.clone();
                        self.load_host(host_id, &path, true);
                        let instrument = &mut self.project.instruments[instrument_id];
                        instrument.plugin_path = plugin.path.clone();
                        instrument.name = plugin.name.clone();
                        // An external generator must not keep the built-in voice
                        // it had before, otherwise it renders as that synth.
                        instrument.synth_kind = SynthKind::External;
                        instrument.synth_enabled = true;
                        instrument.enabled = true;
                        self.open_instrument_plugins.insert(instrument_id);
                        self.plugin_selector_target = None;
                        self.sync_project();
                        break;
                    }
                }
            }
            PluginTarget::Effect { channel, slot } => {
                ui.label("Choose an insert effect");
                let options = [
                    ("3-Band EQ", "3-Band EQ"),
                    ("Reverb", "Reverb"),
                    ("Plate Reverb", "Plate Reverb"),
                    ("Delay", "Delay"),
                    ("Tape Echo", "Tape Echo"),
                    ("Juno Chorus", "Juno Chorus"),
                    ("Phaser", "Phaser"),
                    ("Waveshaper", "Waveshaper"),
                ];
                let current = self.effect_slot_name(channel, slot);
                for (name, label) in options {
                    if !matches(label) {
                        continue;
                    }
                    if ui
                        .selectable_label(current.as_deref() == Some(name), label)
                        .clicked()
                    {
                        if let Some(effect) = self.effect_slot_mut(channel, slot) {
                            effect.name = name.to_owned();
                            effect.plugin_path.clear();
                            effect.enabled = true;
                        }
                        let host_id = pulseforge_core::hosted::effect_host_id(channel, slot);
                        self.unload_host(host_id);
                        self.open_effect_plugins.insert((channel, slot));
                        self.plugin_selector_target = None;
                        self.sync_project();
                        break;
                    }
                }
                if ui.button("Clear insert").clicked() {
                    if let Some(effect) = self.effect_slot_mut(channel, slot) {
                        effect.name.clear();
                        effect.plugin_path.clear();
                        effect.enabled = false;
                    }
                    let host_id = pulseforge_core::hosted::effect_host_id(channel, slot);
                    self.unload_host(host_id);
                    self.plugin_selector_target = None;
                    self.sync_project();
                }
                ui.separator();
                ui.label(egui::RichText::new("Installed plug-ins").strong());
                if self.external_plugins.is_empty() {
                    ui.label("None discovered. Configure folders in Settings ? Plug-ins.");
                }
                for plugin in self.external_plugins.clone() {
                    if !matches(&plugin.label()) {
                        continue;
                    }
                    let active = self
                        .effect_slot_name(channel, slot)
                        .is_some_and(|name| name == plugin.label());
                    if ui
                        .selectable_label(active, plugin.label())
                        .on_hover_text(&plugin.path)
                        .clicked()
                    {
                        if let Some(effect) = self.effect_slot_mut(channel, slot) {
                            effect.name = plugin.label();
                            effect.plugin_path = plugin.path.clone();
                            effect.enabled = true;
                        }
                        let host_id = pulseforge_core::hosted::effect_host_id(channel, slot);
                        let path = plugin.path.clone();
                        self.load_host(host_id, &path, false);
                        self.open_effect_plugins.insert((channel, slot));
                        self.plugin_selector_target = None;
                        self.sync_project();
                        break;
                    }
                }
                if ui.button("Plug-in settings").clicked() {
                    self.windows.settings = true;
                    self.settings_tab = SettingsTab::Plugins;
                }
            }
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

    fn render_automation_window(&mut self, ui: &mut egui::Ui) {
        // Indices can go stale when an instrument is removed while this window
        // is open, which used to panic here.
        self.automation_instrument_id = self
            .automation_instrument_id
            .min(self.project.instruments.len().saturating_sub(1));
        self.automation_channel_id = self.automation_channel_id.min(self.project.lanes.len());
        self.selected_instrument = self
            .selected_instrument
            .min(self.project.instruments.len().saturating_sub(1));
        self.selected_mixer_channel = self.selected_mixer_channel.min(self.project.lanes.len());
        ui.label("Automate every continuous built-in instrument and insert-effect control.");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.automation_effect_mode, false, "Instrument");
            ui.selectable_value(&mut self.automation_effect_mode, true, "Insert FX");
        });

        let target = if self.automation_effect_mode {
            let master_id = self.project.lanes.len();
            self.automation_channel_id = self.automation_channel_id.min(master_id);
            egui::ComboBox::from_id_salt("automation_channel")
                .selected_text(if self.automation_channel_id == master_id {
                    "Master"
                } else {
                    self.project.lanes[self.automation_channel_id].name.as_str()
                })
                .show_ui(ui, |ui| {
                    for (channel_id, lane) in self.project.lanes.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.automation_channel_id,
                            channel_id,
                            &lane.name,
                        );
                    }

                    ui.selectable_value(&mut self.automation_channel_id, master_id, "Master");
                });
            let slots = if self.automation_channel_id == master_id {
                &self.project.master.inserts
            } else {
                &self.project.lanes[self.automation_channel_id].inserts
            };
            let active_slots: Vec<_> = slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| !slot.name.is_empty())
                .map(|(slot_id, slot)| (slot_id, slot.name.clone()))
                .collect();
            if active_slots.is_empty() {
                ui.label("Add an insert effect to this mixer channel first.");
                None
            } else {
                if !active_slots
                    .iter()
                    .any(|(slot_id, _)| *slot_id == self.automation_slot_id)
                {
                    self.automation_slot_id = active_slots[0].0;
                }
                egui::ComboBox::from_id_salt("automation_fx_slot")
                    .selected_text(
                        active_slots
                            .iter()
                            .find(|(slot_id, _)| *slot_id == self.automation_slot_id)
                            .map_or("FX", |(_, name)| name.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        for (slot_id, name) in &active_slots {
                            ui.selectable_value(&mut self.automation_slot_id, *slot_id, name);
                        }
                    });
                let slot_name = &slots[self.automation_slot_id].name;
                let parameters = effect_parameter_options(slot_name);
                if !parameters.contains(&self.automation_effect_parameter) {
                    self.automation_effect_parameter = parameters[0];
                }
                egui::ComboBox::from_id_salt("automation_fx_parameter")
                    .selected_text(self.automation_effect_parameter.label())
                    .show_ui(ui, |ui| {
                        for parameter in parameters {
                            ui.selectable_value(
                                &mut self.automation_effect_parameter,
                                parameter,
                                parameter.label(),
                            );
                        }
                    });
                Some(ParameterTarget::InsertEffect {
                    channel_id: self.automation_channel_id,
                    slot_id: self.automation_slot_id,
                    parameter: self.automation_effect_parameter,
                })
            }
        } else {
            if self.project.instruments.is_empty() {
                ui.label("Add an instrument first.");
                None
            } else {
                self.automation_instrument_id = self
                    .automation_instrument_id
                    .min(self.project.instruments.len() - 1);
                egui::ComboBox::from_id_salt("automation_instrument")
                    .selected_text(&self.project.instruments[self.automation_instrument_id].name)
                    .show_ui(ui, |ui| {
                        for (instrument_id, instrument) in
                            self.project.instruments.iter().enumerate()
                        {
                            ui.selectable_value(
                                &mut self.automation_instrument_id,
                                instrument_id,
                                &instrument.name,
                            );
                        }
                    });
                let synth_kind = self.project.instruments[self.automation_instrument_id].synth_kind;
                let parameters = instrument_parameter_options(synth_kind);
                if !parameters.contains(&self.automation_instrument_parameter) {
                    self.automation_instrument_parameter = parameters[0];
                }
                egui::ComboBox::from_id_salt("automation_instrument_parameter")
                    .selected_text(self.automation_instrument_parameter.label())
                    .show_ui(ui, |ui| {
                        for parameter in parameters {
                            ui.selectable_value(
                                &mut self.automation_instrument_parameter,
                                parameter,
                                parameter.label(),
                            );
                        }
                    });
                Some(ParameterTarget::Instrument {
                    instrument_id: self.automation_instrument_id,
                    parameter: self.automation_instrument_parameter,
                })
            }
        };

        if let Some(target) = target {
            let target_label = parameter_target_label(&self.project, target);
            if ui.button("Add automation lane").clicked()
                && let Some(base_value) = self.project.parameter_value(target)
            {
                let base = target.normalize_value(base_value);
                self.project.automation_tracks.push(AutomationTrack {
                    name: target_label.clone(),
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
        }

        ui.separator();
        // Quick formula tool: an expression is sampled into a real curve lane,
        // so the engine needs no scripting path of its own.
        ui.horizontal(|ui| {
            ui.label("Formula");
            ui.add(
                egui::TextEdit::singleline(&mut self.formula_text)
                    .desired_width(260.0)
                    .hint_text("0.5 + 0.4 * sin(2 * pi * x)"),
            );
        });
        match pulseforge_core::expr::Expression::parse(&self.formula_text) {
            Ok(expression) => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "x=0.25 \u{2192} {:.3} \u{00B7} x=0.5 \u{2192} {:.3} \u{00B7} x=0.75 \u{2192} {:.3}",
                            expression.evaluate(0.25),
                            expression.evaluate(0.5),
                            expression.evaluate(0.75)
                        ))
                        .small()
                        .weak(),
                    );
                    if ui.button("Add formula lane").clicked() {
                        self.add_formula_automation(&expression);
                    }
                });
            }
            Err(error) => {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("formula: {error}"));
            }
        }

        ui.separator();
        let mut changed = false;
        let mut remove_track = None;
        let target_labels: Vec<_> = self
            .project
            .automation_tracks
            .iter()
            .map(|track| parameter_target_label(&self.project, track.target))
            .collect();
        let snap_ticks = self.project.snap_ticks;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (track_id, track) in self.project.automation_tracks.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut track.enabled, "").changed();
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut track.name).desired_width(180.0))
                        .changed();
                    ui.label(&target_labels[track_id]);
                    if ui.small_button("Remove lane").clicked() {
                        remove_track = Some(track_id);
                    }
                });
                let source_label = match track.source {
                    project::AutomationSource::Curve => "Curve",
                    project::AutomationSource::MidiEnvelope { .. } => "MIDI envelope",
                    project::AutomationSource::Lfo { .. } => "LFO",
                    project::AutomationSource::Perlin { .. } => "Perlin",
                };
                egui::ComboBox::from_id_salt(("automation_source", track_id))
                    .selected_text(source_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::Curve),
                                "Curve",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::Curve;
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(
                                    track.source,
                                    project::AutomationSource::MidiEnvelope { .. }
                                ),
                                "MIDI envelope",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::MidiEnvelope {
                                instrument_id: 0,
                                attack: project::TimeValue {
                                    amount: 5.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                decay: project::TimeValue {
                                    amount: 180.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                sustain: 0.7,
                                release: project::TimeValue {
                                    amount: 250.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                base: 0.0,
                                depth: 1.0,
                            };
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::Lfo { .. }),
                                "LFO",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::Lfo {
                                shape: project::LfoShape::Sine,
                                rate: project::TimeValue {
                                    amount: 500.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                retrigger: true,
                                base: 0.5,
                                depth: 0.5,
                            };
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::Perlin { .. }),
                                "Perlin noise",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::Perlin {
                                seed: 1,
                                rate: project::TimeValue {
                                    amount: 8.0,
                                    unit: project::TimeUnit::Steps,
                                },
                                octaves: 4,
                                base: 0.5,
                                depth: 0.5,
                            };
                            changed = true;
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("Clip");
                    let old_start = track.start_step;
                    let start_changed = ui
                        .add(
                            egui::DragValue::new(&mut track.start_step)
                                .range(0..=u64::MAX)
                                .prefix("Start "),
                        )
                        .changed();
                    if start_changed {
                        track.start_step = snap_arrangement_step(track.start_step, snap_ticks);
                    }
                    if track.start_step != old_start {
                        let delta = track.start_step as i128 - old_start as i128;
                        for point in track.points.iter_mut().chain(&mut track.modulation_points) {
                            point.step = (point.step as i128 + delta)
                                .clamp(track.start_step as i128, u64::MAX as i128)
                                as u64;
                        }
                        changed = true;
                    }
                    let length_changed = ui
                        .add(
                            egui::DragValue::new(&mut track.length_steps)
                                .range(1..=u64::MAX)
                                .prefix("Length "),
                        )
                        .changed();
                    if length_changed {
                        let snap_steps = snap_ticks.div_ceil(project::TICKS_PER_STEP).max(1);
                        track.length_steps = track.length_steps.saturating_add(snap_steps / 2)
                            / snap_steps
                            * snap_steps;
                        track.length_steps = track.length_steps.max(snap_steps);
                        changed = true;
                    }
                    let clip_end = track.start_step.saturating_add(track.length_steps);
                    for point in track.points.iter_mut().chain(&mut track.modulation_points) {
                        point.step = point.step.min(clip_end.saturating_sub(1));
                    }
                });
                match &mut track.source {
                    project::AutomationSource::Curve => {}
                    project::AutomationSource::MidiEnvelope {
                        instrument_id,
                        attack,
                        decay,
                        sustain,
                        release,
                        base,
                        depth,
                    } => {
                        if !self.project.instruments.is_empty() {
                            *instrument_id =
                                (*instrument_id).min(self.project.instruments.len() - 1);
                            egui::ComboBox::from_id_salt(("automation_env_instrument", track_id))
                                .selected_text(&self.project.instruments[*instrument_id].name)
                                .show_ui(ui, |ui| {
                                    for (id, instrument) in
                                        self.project.instruments.iter().enumerate()
                                    {
                                        if ui
                                            .selectable_value(instrument_id, id, &instrument.name)
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                    }
                                });
                        }
                        ui.horizontal(|ui| {
                            changed |= edit_time_value(ui, track_id, "Attack", attack);
                            changed |= edit_time_value(ui, track_id, "Decay", decay);
                            changed |= ui
                                .add(egui::Slider::new(sustain, 0.0..=1.0).text("Sustain"))
                                .changed();
                            changed |= edit_time_value(ui, track_id, "Release", release);
                            changed |= ui
                                .add(egui::Slider::new(base, 0.0..=1.0).text("Base"))
                                .changed();
                            changed |= ui
                                .add(egui::Slider::new(depth, 0.0..=1.0).text("Depth"))
                                .changed();
                        });
                    }
                    project::AutomationSource::Lfo {
                        shape,
                        rate,
                        retrigger,
                        base,
                        depth,
                    } => {
                        egui::ComboBox::from_id_salt(("automation_lfo_shape", track_id))
                            .selected_text(format!("{shape:?}"))
                            .show_ui(ui, |ui| {
                                for candidate in [
                                    project::LfoShape::Sine,
                                    project::LfoShape::Triangle,
                                    project::LfoShape::Saw,
                                    project::LfoShape::Square,
                                    project::LfoShape::SampleAndHold,
                                ] {
                                    if ui
                                        .selectable_value(
                                            shape,
                                            candidate,
                                            format!("{candidate:?}"),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        ui.horizontal(|ui| {
                            changed |= edit_time_value(ui, track_id, "Rate", rate);
                            changed |= ui.checkbox(retrigger, "Retrigger").changed();
                            changed |= ui
                                .add(egui::Slider::new(base, 0.0..=1.0).text("Base"))
                                .changed();
                            changed |= ui
                                .add(egui::Slider::new(depth, 0.0..=1.0).text("Depth"))
                                .changed();
                        });
                    }
                    project::AutomationSource::Perlin {
                        seed,
                        rate,
                        octaves,
                        base,
                        depth,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label("Seed");
                            changed |= ui
                                .add(egui::DragValue::new(seed).range(0..=u32::MAX))
                                .changed();
                            changed |= edit_time_value(ui, track_id, "Rate", rate);
                            changed |= ui
                                .add(egui::Slider::new(octaves, 1..=16).text("Octaves"))
                                .changed();
                            changed |= ui
                                .add(egui::Slider::new(base, 0.0..=1.0).text("Base"))
                                .changed();
                            changed |= ui
                                .add(egui::Slider::new(depth, 0.0..=1.0).text("Depth"))
                                .changed();
                        });
                    }
                }
                if !matches!(track.source, project::AutomationSource::Curve) {
                    ui.label("Draw a 0-1 modulation multiplier directly over the lane.");
                }
                ui.separator();
            }
        });
        if let Some(track_id) = remove_track {
            self.project.automation_tracks.remove(track_id);
            changed = true;
        }
        if changed {
            self.sync_project();
        }
    }

    fn render_export_window(&mut self, ui: &mut egui::Ui) {
        ui.label("Render the arrangement offline to stereo 16-bit PCM WAV.");
        ui.horizontal(|ui| {
            ui.label("Output path");
            ui.text_edit_singleline(&mut self.render_path);
        });
        if ui.button("Render").clicked() {
            let path = self.render_path.trim().to_owned();
            if path.is_empty() {
                self.status = "Enter an output path before rendering.".to_owned();
            } else {
                match audio::render_project_to_wav(&self.project, &path) {
                    Ok(frames) => {
                        self.status = format!("Rendered {frames} frames to {path}.");
                    }
                    Err(error) => {
                        self.status = format!("WAV render failed: {error}");
                    }
                }
            }
        }
    }

    /// Sample clip window: file identity, waveform and both the project-wide
    /// and per-clip playback controls, shown side by side.
    fn render_sample_clip_window(&mut self, ui: &mut egui::Ui, lane: usize, clip: usize) {
        let Some(source_path) = self
            .project
            .lanes
            .get(lane)
            .and_then(|lane| lane.clips.get(clip))
            .and_then(|clip| match &clip.source {
                ClipSource::Audio { path } => Some(path.clone()),
                ClipSource::Pattern { .. } => None,
            })
        else {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "That clip is no longer an audio clip.",
            );
            return;
        };
        let file_name = source_path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&source_path)
            .to_owned();
        let stem = file_name
            .rsplit_once('.')
            .map_or(file_name.clone(), |(stem, _)| stem.to_owned());
        let (frames, sample_rate) = audio::read_wav(&source_path)
            .map(|sample| (sample.frames.len(), sample.sample_rate))
            .unwrap_or((0, 48_000));
        let seconds = if sample_rate > 0 {
            frames as f64 / f64::from(sample_rate)
        } else {
            0.0
        };
        ui.horizontal(|ui| {
            ui.heading(stem.clone());
            ui.label(
                egui::RichText::new(&file_name)
                    .small()
                    .color(egui::Color32::from_gray(150)),
            );
        });
        ui.label(format!(
            "{frames} frames \u{00B7} {seconds:.2} s \u{00B7} {sample_rate} Hz"
        ));
        let peaks = self.peaks_for(&source_path, 320);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(320.0), 110.0),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(18));
        if let Some(peaks) = peaks {
            let mid = rect.center().y;
            let width = rect.width() / peaks.len().max(1) as f32;
            for (index, peak) in peaks.iter().enumerate() {
                let x = rect.left() + index as f32 * width;
                let height = (peak.clamp(0.0, 1.0) * rect.height() * 0.45).max(1.0);
                painter.line_segment(
                    [egui::pos2(x, mid - height), egui::pos2(x, mid + height)],
                    egui::Stroke::new(width.max(1.0), egui::Color32::from_rgb(122, 102, 193)),
                );
            }
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waveform unavailable",
                egui::FontId::proportional(12.0),
                egui::Color32::from_gray(140),
            );
        }
        ui.separator();
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("All samples").strong());
                changed |= Knob::new("Volume", 0.0, 1.0, "")
                    .with_default(0.8)
                    .show(ui, &mut self.project.sample_gain);
                changed |= Knob::new("Pan", -1.0, 1.0, "")
                    .with_default(0.0)
                    .show(ui, &mut self.project.sample_pan);
                changed |= Knob::new("Pitch", -24.0, 24.0, " st")
                    .with_default(0.0)
                    .show(ui, &mut self.project.sample_pitch);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("This clip").strong());
                if let Some(clip) = self
                    .project
                    .lanes
                    .get_mut(lane)
                    .and_then(|lane| lane.clips.get_mut(clip))
                {
                    changed |= Knob::new("Volume", 0.0, 1.0, "")
                        .with_default(0.8)
                        .show(ui, &mut clip.sample_gain);
                    changed |= Knob::new("Pan", -1.0, 1.0, "")
                        .with_default(0.0)
                        .show(ui, &mut clip.sample_pan);
                    changed |= Knob::new("Pitch", -24.0, 24.0, " st")
                        .with_default(0.0)
                        .show(ui, &mut clip.sample_pitch);
                }
            });
        });
        if changed {
            self.sync_project();
        }
    }
    /// Adds a mixer channel (and its arrangement lane) and selects it.
    fn add_mixer_channel(&mut self) {
        let old_master_id = self.project.lanes.len();
        self.project.add_lane();
        if self.automation_channel_id == old_master_id {
            self.automation_channel_id = old_master_id + 1;
        }
        self.selected_lane = self.project.lanes.len() - 1;
        self.selected_mixer_channel = self.selected_lane;
        self.sync_project();
    }

    fn render_mixer(&mut self, ui: &mut egui::Ui) {
        profile_scope!("render_mixer");
        ui.horizontal(|ui| {
            ui.label("Mixer");
            if ui
                .checkbox(&mut self.project.mixer_collapsed, "Compact strips")
                .changed()
            {
                self.sync_project();
            }
            if ui.button("+ Mixer channel").clicked() {
                self.add_mixer_channel();
            }
            ui.label("Master stays pinned at the left edge.");
        });
        ui.separator();
        let panel_width = 286.0_f32.min((ui.available_width() * 0.42).max(220.0));
        let strips_width = (ui.available_width() - panel_width - 12.0).max(180.0);
        let content_height = ui.available_height().max(260.0);
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(
                    if self.project.mixer_collapsed {
                        150.0
                    } else {
                        232.0
                    },
                    content_height,
                ),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let master_id = self.project.lanes.len();
                    let master_meter = self.meter_level(master_id);
                    let open_inserts = self.open_effect_plugins.clone();
                    let result = mixer_strip(
                        ui,
                        MixerStrip {
                            channel_id: master_id,
                            name: None,
                            pinned: None,
                            gain: &mut self.project.master.gain,
                            pan: &mut self.project.master.pan,
                            width: None,
                            eq: &mut self.project.master.eq,
                            meter: master_meter,
                            selected: self.selected_mixer_channel == master_id,
                            collapsed: self.project.mixer_collapsed,
                            is_master: true,
                            enabled: true,
                            locked: false,
                            solo: false,
                            inserts: Some(&mut self.project.master.inserts),
                            fx_bypass: None,
                            open_inserts: &open_inserts,
                        },
                    );
                    if result.selected {
                        self.select_channel(master_id);
                    }
                    // Same ordering rule as the lane strips: apply the action,
                    // then push the project so the engine sees the change.
                    for action in result.actions {
                        self.apply_context_action(action);
                    }
                    if result.changed {
                        self.sync_project();
                    }
                },
            );
            ui.separator();
            let lane_strip_width = (strips_width
                - if self.project.mixer_collapsed {
                    150.0
                } else {
                    232.0
                }
                - 8.0)
                .max(120.0);
            ui.allocate_ui_with_layout(
                egui::vec2(lane_strip_width, content_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut order: Vec<_> = (0..self.project.lanes.len()).collect();
                                order.sort_by_key(|lane| !self.project.lanes[*lane].pinned);
                                let mut any_changed = false;
                                let mut actions = Vec::new();
                                for lane_id in order {
                                    let meter = self.meter_level(lane_id);
                                    let open_inserts = self.open_effect_plugins.clone();
                                    let result = {
                                        let lane = &mut self.project.lanes[lane_id];
                                        let (enabled, locked, solo) =
                                            (lane.enabled, lane.locked, lane.solo);
                                        mixer_strip(
                                            ui,
                                            MixerStrip {
                                                channel_id: lane_id,
                                                name: Some(&mut lane.name),
                                                pinned: Some(&mut lane.pinned),
                                                gain: &mut lane.gain,
                                                pan: &mut lane.pan,
                                                width: Some(&mut lane.width),
                                                eq: &mut lane.eq,
                                                meter,
                                                selected: self.selected_mixer_channel == lane_id,
                                                collapsed: self.project.mixer_collapsed,
                                                is_master: false,
                                                enabled,
                                                locked,
                                                solo,
                                                inserts: Some(&mut lane.inserts),
                                                fx_bypass: Some(&mut lane.fx_bypass),
                                                open_inserts: &open_inserts,
                                            },
                                        )
                                    };
                                    if result.selected {
                                        self.selected_mixer_channel = lane_id;
                                        self.selected_lane = lane_id;
                                        self.selection = Some(Selection::MixerChannel(lane_id));
                                    }
                                    any_changed |= result.changed;
                                    actions.extend(result.actions);
                                }
                                // Actions first: they mutate the model (insert
                                // enable, solo, favourites), and the sync below is
                                // what actually reaches the audio engine.
                                for action in actions {
                                    self.apply_context_action(action);
                                }
                                if any_changed {
                                    self.sync_project();
                                }
                                // Add-track affordance at the end of the row, so
                                // channels can be added where they appear. Right
                                // click adds one pre-loaded with a saved chain.
                                let add_track = ui
                                    .add_sized(
                                        [86.0, 34.0],
                                        egui::Button::new("+ Track")
                                            .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text(
                                        "Left click: add a channel. Right click: add one from a preset.",
                                    );
                                let mut add_from_preset: Option<String> = None;
                                add_track.context_menu(|ui| {
                                    ui.label("Add channel from preset");
                                    if self.chain_presets.is_empty() {
                                        ui.label("No presets saved yet.");
                                    }
                                    for name in self.chain_presets.clone() {
                                        if ui.button(&name).clicked() {
                                            add_from_preset = Some(name);
                                            ui.close();
                                        }
                                    }
                                });
                                if add_track.clicked() {
                                    self.add_mixer_channel();
                                }
                                if let Some(name) = add_from_preset {
                                    self.add_mixer_channel();
                                    let channel = self.selected_mixer_channel;
                                    match load_chain_preset(&name) {
                                        Ok(preset) => {
                                            if let Some(lane) =
                                                self.project.lanes.get_mut(channel)
                                            {
                                                for (slot, loaded) in
                                                    lane.inserts.iter_mut().zip(preset)
                                                {
                                                    *slot = loaded;
                                                }
                                            }
                                            self.sync_project();
                                            // Instantiate the chain's plug-ins so
                                            // their saved state is applied.
                                            self.restore_hosted_plugins();
                                            self.status = format!(
                                                "Added a channel with preset '{name}'."
                                            );
                                        }
                                        Err(error) => self.status = error,
                                    }
                                }
                            });
                        });
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(panel_width, content_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.render_mixer_inspector(ui),
            );
        });
    }

    fn render_mixer_inspector(&mut self, ui: &mut egui::Ui) {
        let channel = self.selected_mixer_channel.min(self.project.lanes.len());
        let name = if channel == self.project.lanes.len() {
            "MASTER".to_owned()
        } else {
            self.project.lanes[channel].name.clone()
        };
        ui.heading(format!("Track · {name}"));
        ui.separator();
        let is_master = channel == self.project.lanes.len();
        // Level and pan live on the strips, so the inspector shows what a strip
        // cannot: EQ, stereo width and the insert chain, on one page.
        let (eq, inserts, width) = if is_master {
            (
                &mut self.project.master.eq,
                &mut self.project.master.inserts,
                None,
            )
        } else {
            let lane = &mut self.project.lanes[channel];
            (&mut lane.eq, &mut lane.inserts, Some(&mut lane.width))
        };
        let mut changed = false;
        ui.label(egui::RichText::new("3-BAND EQ").strong());
        draw_eq_curve(ui, *eq);
        ui.horizontal(|ui| {
            changed |= Knob::new("LOW", -12.0, 12.0, " dB").show(ui, &mut eq.low_db);
            changed |= Knob::new("MID", -12.0, 12.0, " dB").show(ui, &mut eq.mid_db);
            changed |= Knob::new("HIGH", -12.0, 12.0, " dB").show(ui, &mut eq.high_db);
            if let Some(width) = width {
                changed |= Knob::new("WIDTH", 0.0, 2.0, "")
                    .with_default(1.0)
                    .show(ui, width);
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("INSERTS").strong());
        // Chain presets save and restore the whole insert list as one JSON file,
        // so a channel strip can be reused on another lane or in another project.
        // Save/load are deferred past the borrow of `inserts` below, because
        // capturing hosted plug-in state and re-instantiating it both need `self`.
        let mut save_preset_request: Option<String> = None;
        let mut preset_loaded = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.chain_preset_name)
                    .desired_width(120.0)
                    .hint_text("Preset name"),
            );
            if ui.button("Save chain").clicked() {
                save_preset_request = Some(self.chain_preset_name.clone());
            }
            ui.menu_button("Load chain", |ui| {
                if self.chain_presets.is_empty() {
                    ui.label("No presets saved yet.");
                }
                let names = self.chain_presets.clone();
                for name in names {
                    if ui.button(&name).clicked() {
                        match load_chain_preset(&name) {
                            Ok(preset) => {
                                for (slot, loaded) in inserts.iter_mut().zip(preset) {
                                    *slot = loaded;
                                }
                                changed = true;
                                preset_loaded = true;
                                self.status = format!("Loaded chain preset '{name}'.");
                            }
                            Err(error) => self.status = error,
                        }
                        ui.close();
                    }
                }
            });
        });
        {
            ui.horizontal(|ui| {
                if ui.button("Add insert effect").clicked() {
                    if let Some(slot_id) = inserts.iter().position(|slot| slot.name.is_empty()) {
                        self.plugin_selector_target = Some(PluginTarget::Effect {
                            channel,
                            slot: slot_id,
                        });
                    } else {
                        self.status = "All insert slots are occupied.".to_owned();
                    }
                }
                if ui.button("Choose plugin…").clicked() {
                    self.plugin_selector_target = Some(PluginTarget::Effect { channel, slot: 0 });
                }
            });
            for (slot_id, slot) in inserts.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let name = if slot.name.is_empty() {
                        "Empty"
                    } else {
                        &slot.name
                    };
                    if ui.button(format!("{} · {name}", slot_id + 1)).clicked() {
                        self.open_effect_plugins.insert((channel, slot_id));
                    }
                    if ui.small_button("Select…").clicked() {
                        self.plugin_selector_target = Some(PluginTarget::Effect {
                            channel,
                            slot: slot_id,
                        });
                    }
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        if slot.enabled {
                            egui::Color32::from_rgb(60, 120, 110)
                        } else {
                            egui::Color32::from_gray(35)
                        },
                    );
                    paint_icon(
                        ui.painter(),
                        rect.shrink(3.0),
                        Icon::Power,
                        if slot.enabled {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::from_gray(110)
                        },
                    );
                    if slot.locked {
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left() + 2.0, rect.bottom() - 2.0),
                                egui::pos2(rect.right() - 2.0, rect.bottom() - 2.0),
                            ],
                            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(230, 178, 92)),
                        );
                    }
                    tooltip(ui, &response, "Power. Shift+click locks, ctrl+click solos");
                    if response.clicked() {
                        if self.mod_shift {
                            slot.locked = !slot.locked;
                        } else if self.mod_ctrl {
                            // Ctrl+click solos this insert, muting the other
                            // inserts on the same channel.
                            slot.solo = !slot.solo;
                        } else {
                            // The lock only shields the slot from global
                            // operations; a direct click still bypasses it.
                            slot.enabled = !slot.enabled;
                        }
                        changed = true;
                    }
                    if slot.enabled && ui.small_button("Remove").clicked() {
                        slot.enabled = false;
                        slot.name.clear();
                        changed = true;
                    }
                });
            }
        }
        if let Some(name) = save_preset_request {
            // External plug-ins hold their parameters in the hosted instance, so
            // the opaque state has to be captured before writing the chain out.
            self.capture_plugin_states();
            let slots: &[InsertFxSlot] = if is_master {
                &self.project.master.inserts
            } else {
                &self.project.lanes[channel].inserts
            };
            match save_chain_preset(&name, slots) {
                Ok(name) => {
                    self.status = format!("Saved chain preset '{name}'.");
                    self.chain_presets = list_chain_presets();
                }
                Err(error) => self.status = error,
            }
        }
        if preset_loaded {
            // Re-instantiate the hosted plug-ins so their saved state applies.
            self.restore_hosted_plugins();
        }
        if changed {
            self.sync_project();
        }
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

    fn apply_context_action(&mut self, action: ContextAction) {
        match action {
            ContextAction::ToggleLanePin(lane) => {
                if let Some(lane) = self.project.lanes.get_mut(lane) {
                    lane.pinned = !lane.pinned;
                }
            }
            ContextAction::DisconnectChannel(channel) => {
                disconnect_channel(&mut self.project.sends, channel);
            }
            ContextAction::SelectChannel(channel) => {
                if channel <= self.project.lanes.len() {
                    self.selected_mixer_channel = channel;
                    self.selection = Some(Selection::MixerChannel(channel));
                    if channel < self.project.lanes.len() {
                        self.selected_lane = channel;
                    }
                }
            }
            ContextAction::DeleteClip { lane, clip } => {
                if let Some(lane) = self.project.lanes.get_mut(lane)
                    && clip < lane.clips.len()
                {
                    lane.clips.remove(clip);
                    self.selection = None;
                }
            }
            ContextAction::DuplicateClip { lane, clip } => {
                self.duplicate_clip(lane, clip);
                return;
            }
            ContextAction::MakeClipPatternUnique { lane, clip } => {
                self.make_clip_pattern_unique(lane, clip);
                return;
            }
            ContextAction::RenameTrack(lane) => {
                self.selected_lane = lane;
                self.begin_rename_track();
                return;
            }
            ContextAction::MoveTrack { lane, up } => {
                let target = if up {
                    lane.checked_sub(1)
                } else {
                    Some(lane + 1).filter(|target| *target < self.project.lanes.len())
                };
                if let Some(target) = target {
                    self.push_history();
                    self.project.move_lane(lane, target);
                    self.selected_lane = target;
                    self.selection = Some(Selection::MixerChannel(target));
                }
            }
            ContextAction::ToggleTrackAutomation(lane) => {
                if let Some(track) = self.project.lanes.get_mut(lane) {
                    track.automation_collapsed = !track.automation_collapsed;
                }
            }
            ContextAction::DeleteAutomation(track_id) => {
                if track_id < self.project.automation_tracks.len() {
                    self.project.automation_tracks.remove(track_id);
                    self.selection = None;
                }
            }
            ContextAction::DuplicateAutomation(track_id) => {
                if let Some(track) = self.project.automation_tracks.get(track_id).cloned() {
                    // Shift the copy after the original so both clips are
                    // visible side by side on the same parameter.
                    let mut copy = track;
                    copy.start_step = copy.start_step.saturating_add(copy.length_steps.max(1));
                    copy.name = format!("{} copy", copy.name);
                    self.push_history();
                    self.project.automation_tracks.push(copy);
                }
            }
            ContextAction::NewAutomationForTrack(lane) => {
                let target = ParameterTarget::InsertEffect {
                    channel_id: lane,
                    slot_id: 0,
                    parameter: EffectParameter::Mix,
                };
                self.push_history();
                self.project.automation_tracks.push(AutomationTrack {
                    name: format!(
                        "{} ? Insert 1 mix",
                        self.project
                            .lanes
                            .get(lane)
                            .map_or("Track", |track| track.name.as_str())
                    ),
                    target,
                    start_step: self.arrangement_pan_steps,
                    length_steps: 32,
                    points: vec![
                        AutomationPoint {
                            step: 0,
                            value: 0.2,
                            curve: AutomationCurve::Linear,
                        },
                        AutomationPoint {
                            step: 32,
                            value: 0.8,
                            curve: AutomationCurve::Smooth,
                        },
                    ],
                    modulation_points: Vec::new(),
                    enabled: true,
                    source: project::AutomationSource::Curve,
                });
            }
            ContextAction::RenameInstrument(instrument) => {
                self.renaming_instrument = Some(instrument);
                self.rename_buffer = self
                    .project
                    .instruments
                    .get(instrument)
                    .map_or(String::new(), |item| item.name.clone());
                return;
            }
            ContextAction::RemoveInstrument(instrument) => {
                if self.project.instruments.len() > 1 && instrument < self.project.instruments.len()
                {
                    self.push_history();
                    let host_id = pulseforge_core::hosted::instrument_host_id(instrument);
                    self.unload_host(host_id);
                    self.project.instruments.remove(instrument);
                    self.open_instrument_plugins.remove(&instrument);
                    // Host ids are instrument indices, so every id above the
                    // removed one shifts down.
                    let mut hosted = std::mem::take(&mut self.hosted);
                    let mut meta = std::mem::take(&mut self.hosted_meta);
                    fn renumber<T>(map: &mut std::collections::HashMap<u64, T>, removed: u64) {
                        let moved: Vec<(u64, T)> = map
                            .drain()
                            .filter_map(|(id, value)| {
                                (id != removed)
                                    .then_some((if id > removed { id - 1 } else { id }, value))
                            })
                            .collect();
                        for (id, value) in moved {
                            map.insert(id, value);
                        }
                    }
                    renumber(&mut hosted, host_id);
                    renumber(&mut meta, host_id);
                    self.hosted = hosted;
                    self.hosted_meta = meta;
                    self.attached_editor = None;
                    self.selected_instrument = self
                        .selected_instrument
                        .min(self.project.instruments.len() - 1);
                    self.selection = None;
                    // Every remaining reference to an instrument is by index, so
                    // they all shift with the removal. A dangling id used to make
                    // the engine and the automation UI index past the end.
                    let shift = |route: &mut usize| {
                        if *route == instrument {
                            *route = instrument.saturating_sub(1);
                        } else if *route > instrument {
                            *route -= 1;
                        }
                    };
                    for route in &mut self.project.midi_routes {
                        shift(route);
                    }
                    if let Ok(mut routes) = self.midi_routes.lock() {
                        for route in routes.iter_mut() {
                            shift(route);
                        }
                    }
                    self.project.automation_tracks.retain_mut(|track| {
                        if let ParameterTarget::Instrument { instrument_id, .. } =
                            &mut track.target
                        {
                            if *instrument_id == instrument {
                                return false;
                            }
                            if *instrument_id > instrument {
                                *instrument_id -= 1;
                            }
                        }
                        if let project::AutomationSource::MidiEnvelope { instrument_id, .. } =
                            &mut track.source
                        {
                            if *instrument_id == instrument {
                                return false;
                            }
                            if *instrument_id > instrument {
                                *instrument_id -= 1;
                            }
                        }
                        true
                    });
                    let last = self.project.instruments.len().saturating_sub(1);
                    for route in &mut self.project.midi_routes {
                        *route = (*route).min(last);
                    }
                    if let Ok(mut routes) = self.midi_routes.lock() {
                        for route in routes.iter_mut() {
                            *route = (*route).min(last);
                        }
                    }
                } else {
                    self.status = "A project needs at least one instrument.".to_owned();
                }
            }
            ContextAction::SetInstrumentColor { instrument, color } => {
                if let Some(item) = self.project.instruments.get_mut(instrument) {
                    item.color = color;
                }
            }
            ContextAction::OpenInstrumentPicker(instrument) => {
                self.plugin_selector_target = Some(PluginTarget::Instrument(instrument));
                self.plugin_search.clear();
                return;
            }
            ContextAction::ToggleChannelEnabled(channel) => {
                let solo = self.mod_ctrl;
                let lock = self.mod_shift;
                if channel < self.project.lanes.len() {
                    let lane = &mut self.project.lanes[channel];
                    if lock {
                        lane.locked = !lane.locked;
                    } else if solo {
                        lane.solo = !lane.solo;
                    } else {
                        lane.enabled = !lane.enabled;
                    }
                }
            }
            ContextAction::ToggleInsertEnabled {
                channel,
                slot,
                intent,
            } => {
                if let Some(effect) = self.effect_slot_mut(channel, slot) {
                    if intent.lock {
                        effect.locked = !effect.locked;
                    } else if intent.solo {
                        effect.solo = !effect.solo;
                    } else if intent.toggle {
                        effect.enabled = !effect.enabled;
                    }
                }
            }
            ContextAction::OpenInsertPicker { channel, slot } => {
                self.plugin_selector_target = Some(PluginTarget::Effect { channel, slot });
                self.plugin_search.clear();
                return;
            }
            ContextAction::OpenInsertWindow { channel, slot } => {
                let has_slot = self
                    .effect_slot_name(channel, slot)
                    .is_some_and(|name| !name.is_empty());
                if has_slot {
                    self.open_effect_plugins.insert((channel, slot));
                } else {
                    self.plugin_selector_target = Some(PluginTarget::Effect { channel, slot });
                }
                return;
            }
            ContextAction::ClearInsert { channel, slot } => {
                if let Some(effect) = self.effect_slot_mut(channel, slot) {
                    effect.name.clear();
                    effect.plugin_path.clear();
                    effect.enabled = false;
                    effect.solo = false;
                }
                let host_id = pulseforge_core::hosted::effect_host_id(channel, slot);
                self.unload_host(host_id);
                self.open_effect_plugins.remove(&(channel, slot));
            }
            ContextAction::OpenPluginSettings => {
                self.windows.settings = true;
                self.settings_tab = SettingsTab::Plugins;
                return;
            }
        }
        self.sync_project();
    }

    fn duplicate_selected_clip(&mut self) {
        if let Some(Selection::Clip { lane, clip }) = self.selection {
            self.duplicate_clip(lane, clip);
        } else {
            self.status = "Select an arrangement clip to duplicate.".to_owned();
        }
    }

    fn delete_selected_clip(&mut self) {
        if let Some(Selection::Clip { lane, clip }) = self.selection
            && let Some(lane) = self.project.lanes.get_mut(lane)
            && clip < lane.clips.len()
        {
            lane.clips.remove(clip);
            self.selection = None;
            self.sync_project();
        }
    }

    fn duplicate_clip(&mut self, lane: usize, clip: usize) {
        let audio_path = self
            .project
            .lanes
            .get(lane)
            .and_then(|lane| lane.clips.get(clip))
            .and_then(|clip| match &clip.source {
                ClipSource::Audio { path } => Some(path.clone()),
                ClipSource::Pattern { .. } => None,
            });
        let Some(duplicate_id) = self.project.duplicate_clip_after(lane, clip) else {
            self.status = "That arrangement clip is no longer available.".to_owned();
            return;
        };
        self.selected_lane = lane;
        self.selection = Some(Selection::Clip {
            lane,
            clip: duplicate_id,
        });
        self.sync_project();

        if let Some(path) = audio_path {
            match audio::read_wav(&path) {
                Ok(sample) => {
                    let frame_count = sample.frames.len();
                    if self.send_audio(AudioCommand::LoadAudioClip {
                        path: path.clone(),
                        sample,
                    }) {
                        self.status = format!("Duplicated audio clip ({frame_count} frames).");
                    }
                }
                Err(error) => {
                    self.status = format!("Clip duplicated but WAV reload failed: {error}");
                }
            }
        } else {
            self.status = "Duplicated the clip after its original.".to_owned();
        }
    }

    fn make_clip_pattern_unique(&mut self, lane: usize, clip: usize) {
        let Some(pattern_id) = self.project.make_clip_pattern_unique(lane, clip) else {
            self.status = "Only pattern clips can be made unique.".to_owned();
            return;
        };
        self.selected_lane = lane;
        self.selected_pattern = pattern_id;
        self.selection = Some(Selection::Clip { lane, clip });
        self.sync_project();
        self.status = "Made this clip's pattern unique.".to_owned();
    }

    fn meter_level(&self, channel: usize) -> f32 {
        self.audio
            .as_ref()
            .and_then(|audio| {
                audio
                    .meters
                    .try_lock()
                    .ok()
                    .and_then(|values| values.get(channel).copied())
            })
            .unwrap_or(0.0)
    }

    fn render_routing_cables(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        profile_scope!("render_routing_cables");
        ui.horizontal(|ui| {
            ui.heading("Routing cables");
            ui.label(
                "Drag outputs to inputs · hover a cable for level · right-click to disconnect",
            );
        });
        let lane_count = self.project.lanes.len();
        let channel_count = lane_count + 1;
        let mut order = vec![lane_count];
        let mut lanes: Vec<_> = (0..lane_count).collect();
        lanes.sort_by_key(|lane| !self.project.lanes[*lane].pinned);
        order.extend(lanes);
        let canvas_height = 48.0 + channel_count as f32 * 38.0;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(420.0), canvas_height),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 5.0, egui::Color32::from_gray(22));
        let source_x = rect.left() + 156.0;
        let target_x = rect.right() - 156.0;
        painter.text(
            egui::pos2(rect.left() + 18.0, rect.top() + 18.0),
            egui::Align2::LEFT_CENTER,
            "CHANNEL OUTPUTS",
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(155),
        );
        painter.text(
            egui::pos2(rect.right() - 18.0, rect.top() + 18.0),
            egui::Align2::RIGHT_CENTER,
            "CHANNEL INPUTS",
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(155),
        );
        let channel_name = |index: usize, project: &Project| {
            if index == project.lanes.len() {
                "MASTER".to_owned()
            } else {
                project.lanes[index].name.clone()
            }
        };
        let mut node_y = vec![0.0; channel_count];
        for (row, channel) in order.iter().copied().enumerate() {
            node_y[channel] = rect.top() + 48.0 + row as f32 * 38.0;
        }
        let channel_color = |index: usize, project: &Project| {
            if index == project.lanes.len() {
                egui::Color32::WHITE
            } else {
                lane_color(index)
            }
        };
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let mut nearest_route = None;
        let mut nearest_distance = f32::INFINITY;
        let mut routes = Vec::new();
        for source in 0..channel_count {
            for destination in 0..channel_count {
                let gain = self.project.sends[source][destination];
                if gain <= 0.001 {
                    continue;
                }
                let start = egui::pos2(source_x, node_y[source]);
                let end = egui::pos2(target_x, node_y[destination]);
                let control_x = (start.x + end.x) * 0.5;
                let control = egui::pos2(control_x, (start.y + end.y) * 0.5);
                let mut points = Vec::with_capacity(25);
                for segment in 0..=24 {
                    let t = segment as f32 / 24.0;
                    let one_minus_t = 1.0 - t;
                    points.push(egui::pos2(
                        one_minus_t * one_minus_t * start.x
                            + 2.0 * one_minus_t * t * control.x
                            + t * t * end.x,
                        one_minus_t * one_minus_t * start.y
                            + 2.0 * one_minus_t * t * control.y
                            + t * t * end.y,
                    ));
                }
                if let Some(pointer) = pointer.filter(|point| rect.contains(*point)) {
                    let distance = points
                        .windows(2)
                        .map(|pair| point_segment_distance(pointer, pair[0], pair[1]))
                        .fold(f32::INFINITY, f32::min);
                    if distance < nearest_distance {
                        nearest_distance = distance;
                        nearest_route = Some((source, destination, points.clone()));
                    }
                }
                routes.push((source, destination, gain, points));
            }
        }
        let hovered_route = (nearest_distance < 13.0).then_some(nearest_route).flatten();
        for (source, destination, gain, points) in &routes {
            for (segment, pair) in points.windows(2).enumerate() {
                let blend = segment as f32 / (points.len() - 1) as f32;
                painter.line_segment(
                    [pair[0], pair[1]],
                    egui::Stroke::new(
                        if hovered_route
                            .as_ref()
                            .is_some_and(|(from, to, _)| from == source && to == destination)
                        {
                            3.5
                        } else {
                            1.2 + *gain * 2.0
                        },
                        color_lerp(
                            channel_color(*source, &self.project),
                            channel_color(*destination, &self.project),
                            blend,
                        ),
                    ),
                );
            }
        }

        for index in order.iter().copied() {
            let y = node_y[index];
            let name = channel_name(index, &self.project);
            let color = channel_color(index, &self.project);
            painter.text(
                egui::pos2(rect.left() + 18.0, y),
                egui::Align2::LEFT_CENTER,
                &name,
                egui::FontId::proportional(12.0),
                color,
            );
            painter.text(
                egui::pos2(rect.right() - 18.0, y),
                egui::Align2::RIGHT_CENTER,
                &name,
                egui::FontId::proportional(12.0),
                color,
            );
            painter.circle_filled(egui::pos2(source_x, y), 6.0, color);
            painter.circle_filled(egui::pos2(target_x, y), 6.0, color);
        }

        let (pressed, secondary_pressed, released, secondary_released) = ui.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.pointer.button_released(egui::PointerButton::Primary),
                input
                    .pointer
                    .button_released(egui::PointerButton::Secondary),
            )
        });
        let mut changed = false;
        if secondary_pressed && let Some(pointer) = pointer.filter(|point| rect.contains(*point)) {
            let node = order.iter().copied().find(|index| {
                pointer.distance(egui::pos2(source_x, node_y[*index])) <= 12.0
                    || pointer.distance(egui::pos2(target_x, node_y[*index])) <= 12.0
            });
            if let Some(channel) = node {
                disconnect_channel(&mut self.project.sends, channel);
                changed = true;
            } else if let Some((source, destination, _)) = &hovered_route {
                self.project.sends[*source][*destination] = 0.0;
                changed = true;
            }
        }
        if pressed && let Some(pointer) = pointer.filter(|point| rect.contains(*point)) {
            self.routing_drag_from = order
                .iter()
                .copied()
                .find(|index| pointer.distance(egui::pos2(source_x, node_y[*index])) <= 12.0);
        }
        if let Some(source) = self.routing_drag_from
            && let Some(pointer) = pointer.filter(|point| rect.contains(*point))
        {
            let end = egui::pos2(pointer.x, pointer.y);
            painter.line_segment(
                [egui::pos2(source_x, node_y[source]), end],
                egui::Stroke::new(2.0_f32, channel_color(source, &self.project)),
            );
            if released {
                if let Some(destination) = order
                    .iter()
                    .copied()
                    .find(|index| pointer.distance(egui::pos2(target_x, node_y[*index])) <= 12.0)
                {
                    if self.project.sends[source][destination] <= 0.001 {
                        self.project.sends[source][destination] = 0.25;
                        changed = true;
                    }
                }
            }
        }
        if released || secondary_released {
            self.routing_drag_from = None;
        }

        if let Some((source, destination)) = hovered_route
            .map(|(source, destination, _)| (source, destination))
            .or(self.routing_gain_route)
        {
            let middle = {
                let start = egui::pos2(source_x, node_y[source]);
                let end = egui::pos2(target_x, node_y[destination]);
                let control = egui::pos2((start.x + end.x) * 0.5, (start.y + end.y) * 0.5);
                // Midpoint of the quadratic curve at t = 0.5.
                egui::pos2(
                    0.25 * start.x + 0.5 * control.x + 0.25 * end.x,
                    0.25 * start.y + 0.5 * control.y + 0.25 * end.y,
                )
            };
            let knob_rect = egui::Rect::from_center_size(middle, egui::vec2(34.0, 34.0));
            // The send level uses the shared knob widget, so it drags, resets and
            // offers its context menu exactly like every other parameter.
            let mut value = self.project.sends[source][destination];
            ui.scope_builder(egui::UiBuilder::new().max_rect(knob_rect), |ui| {
                if Knob::new("", 0.0, 0.95, "")
                    .with_default(0.25)
                    .compact()
                    .show(ui, &mut value)
                {
                    self.project.sends[source][destination] = value;
                    changed = true;
                }
            });
        }

        for channel in order.iter().copied() {
            let node_rect = egui::Rect::from_center_size(
                egui::pos2(source_x, node_y[channel]),
                egui::vec2(24.0, 24.0),
            );
            let node_response = ui.interact(
                node_rect,
                egui::Id::new(("route_output", channel)),
                egui::Sense::click(),
            );
            node_response.on_hover_text("Drag to a channel input to create a send");
        }
        let _response = response;
        ui.label("Active sends");
        for (source, destination, _, _) in &routes {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{}  ->  {}",
                    channel_name(*source, &self.project),
                    channel_name(*destination, &self.project)
                ));
                let mut gain = self.project.sends[*source][*destination];
                if ui
                    .add(egui::Slider::new(&mut gain, 0.0..=0.95).show_value(true))
                    .changed()
                {
                    self.project.sends[*source][*destination] = gain;
                    changed = true;
                }
                if ui.small_button("Remove").clicked() {
                    self.project.sends[*source][*destination] = 0.0;
                    changed = true;
                }
            });
        }
        if changed {
            self.project.normalize();
            self.sync_project();
        }
        context.request_repaint_after(std::time::Duration::from_millis(40));
    }

    fn render_midi(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Refresh inputs").clicked() {
                match Self::query_midi_ports() {
                    Ok(ports) => {
                        self.midi_ports = ports;
                        self.midi_discovery_error = None;
                    }
                    Err(error) => self.midi_discovery_error = Some(error),
                }
            }
            let selected_name = self
                .selected_midi_port
                .and_then(|id| self.midi_ports.get(id))
                .map_or("Select input", String::as_str);
            egui::ComboBox::from_id_salt("midi_input")
                .selected_text(selected_name)
                .show_ui(ui, |ui| {
                    for (id, name) in self.midi_ports.clone().iter().enumerate() {
                        if ui
                            .selectable_label(self.selected_midi_port == Some(id), name)
                            .clicked()
                        {
                            self.connect_midi(id);
                        }
                    }
                });
            if self.midi_connection.is_some() && ui.button("Disconnect").clicked() {
                self.midi_connection.take();
                self.selected_midi_port = None;
            }
        });
        if let Some(error) = &self.midi_discovery_error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        ui.separator();
        egui::Grid::new("midi_route_grid")
            .striped(true)
            .show(ui, |ui| {
                ui.label("Input channel");
                ui.label("Instrument");
                ui.end_row();
                for channel in 0..MIDI_CHANNEL_COUNT {
                    ui.label(format!("Channel {}", channel + 1));
                    let mut instrument = self.project.midi_routes[channel];
                    egui::ComboBox::from_id_salt(("route", channel))
                        .selected_text(
                            self.project
                                .instruments
                                .get(instrument)
                                .map_or("Instrument", |item| item.name.as_str())
                                .to_owned(),
                        )
                        .show_ui(ui, |ui| {
                            for (id, item) in self.project.instruments.iter().enumerate() {
                                ui.selectable_value(&mut instrument, id, &item.name);
                            }
                        });
                    if instrument != self.project.midi_routes[channel] {
                        self.project.midi_routes[channel] = instrument;
                        self.sync_project();
                    }
                    ui.end_row();
                }
            });
    }

    fn render_footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.project_path)
                    .desired_width(230.0)
                    .hint_text("session.json"),
            );
            if ui.button("Save").clicked() {
                self.save_project();
            }
            if ui.button("Load").clicked() {
                self.load_project();
            }
            ui.separator();
            ui.label(format!("Selected: {}", self.selection_label()));
            ui.separator();
            if let Some(audio) = &self.audio {
                ui.label(format!("{} Hz", audio.sample_rate));
            } else if let Some(error) = &self.audio_error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            // Background jobs are visible, so a slow scan never looks like a
            // hang.
            let running = self.tasks.running();
            if running > 0 {
             
            // Live machine/engine numbers while the debug panel is open.
            if !self.debug_summary.is_empty() {
                ui.separator();
                ui.label(
                    egui::RichText::new(self.debug_summary.clone())
                        .small()
                        .weak(),
                );
            }   ui.label(format!("{running} background job(s)"));
            }
            ui.separator();
            // Hovered control details: name, value and unit (tooltip replacement).
            let hovered = ui.data(|data| {
                data.get_temp::<String>(egui::Id::new(HOVER_TEXT_KEY))
                    .filter(|text| !text.is_empty())
            });
            if let Some(text) = hovered {
                ui.label(
                    egui::RichText::new(text)
                        .color(self.ui_accent())
                        .monospace(),
                );
                ui.separator();
            }
            let live = self.status.clone();
            ui.label(live);
        });
    }

    fn selection_label(&self) -> String {
        match self.selection {
            Some(Selection::Note {
                pattern,
                instrument,
                pitch,
                start_step,
            }) => self
                .project
                .patterns
                .get(pattern)
                .and_then(|pattern| {
                    pattern.notes.iter().find(|note| {
                        note.instrument_id == instrument
                            && note.pitch == pitch
                            && note.start_step == start_step
                    })
                })
                .map_or_else(
                    || "note".to_owned(),
                    |note| format!("note {} @ {}", note_name(note.pitch), note.start_step),
                ),
            Some(Selection::Instrument(instrument)) => self
                .project
                .instruments
                .get(instrument)
                .map_or_else(|| "instrument".to_owned(), |item| item.name.clone()),
            Some(Selection::Clip { lane, clip }) => self
                .project
                .lanes
                .get(lane)
                .and_then(|lane| lane.clips.get(clip))
                .map_or_else(
                    || "clip".to_owned(),
                    |item| match &item.source {
                        ClipSource::Pattern { pattern_id } => {
                            self.project.patterns.get(*pattern_id).map_or_else(
                                || "pattern clip".to_owned(),
                                |pattern| pattern.name.clone(),
                            )
                        }
                        ClipSource::Audio { path } => std::path::Path::new(path)
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .unwrap_or("audio clip")
                            .to_owned(),
                    },
                ),
            Some(Selection::MixerChannel(channel)) => self
                .project
                .lanes
                .get(channel)
                .map_or_else(|| "MASTER".to_owned(), |lane| lane.name.clone()),
            Some(Selection::Automation(track)) => self
                .project
                .automation_tracks
                .get(track)
                .map_or_else(|| "automation".to_owned(), |track| track.name.clone()),
            None => "none".to_owned(),
        }
    }
}

impl PulseForgeApp {
    /// Current automated values for every automated parameter, used to show
    /// automation movement on knobs while transport is running.
    fn live_automated_values(&self) -> Vec<(ParameterTarget, f32)> {
        let Some(audio) = &self.audio else {
            return Vec::new();
        };
        if !self.playing {
            return Vec::new();
        }
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
        for track in &self.project.automation_tracks {
            if !track.enabled
                || track.target != target
                || position < track.start_step as f64
                || position >= track.start_step.saturating_add(track.length_steps) as f64
            {
                continue;
            }
            if matches!(track.source, project::AutomationSource::Curve) {
                if track.points.is_empty() {
                    continue;
                }
                let value = Project::evaluate_curve(&track.points, position);
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
            normalized =
                Some(normalized.unwrap_or(base_normalized) + contribution - base_normalized);
        }
        normalized.map(|value| target.denormalize_value(value.clamp(0.0, 1.0)))
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
    /// Settings window. Every page edits [`Settings`] directly, and changes that
    /// affect the audio engine are pushed as soon as they are made.
    fn render_settings_window(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (SettingsTab::Shortcuts, "Shortcuts"),
                (SettingsTab::Plugins, "Plug-ins"),
                (SettingsTab::Samples, "Samples"),
                (SettingsTab::Interface, "Interface"),
                (SettingsTab::Graphics, "Graphics"),
                (SettingsTab::Theming, "Theming"),
                (SettingsTab::General, "General"),
            ] {
                if ui
                    .selectable_label(self.settings_tab == tab, label)
                    .clicked()
                {
                    self.settings_tab = tab;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Search settings");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings_search)
                    .desired_width(220.0)
                    .hint_text("Option name"),
            );
            if ui.small_button("Clear").clicked() {
                self.settings_search.clear();
            }
        });
        ui.separator();
        match self.settings_tab {
            SettingsTab::Shortcuts => self.render_shortcut_settings(ui),
            SettingsTab::Plugins => self.render_plugin_settings(ui),
            SettingsTab::Samples => self.render_sample_settings(ui),
            SettingsTab::Interface => self.render_interface_settings(ui),
            SettingsTab::Graphics => self.render_graphics_settings(ui),
            SettingsTab::Theming => self.render_theming_settings(ui),
            SettingsTab::General => self.render_general_settings(ui),
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Save settings").clicked() {
                self.save_settings();
            }
            if ui.button("Reload").clicked() {
                self.settings = Settings::load(&self.settings_path);
                self.apply_audio_settings();
                self.status = "Settings reloaded.".to_owned();
            }
            ui.label(format!("File: {}", self.settings_path));
        });
    }

    /// Editable key map. Clicking a chord arms capture; the next key press is
    /// assigned through `Settings::bind`, which is the same API code uses.
    fn render_shortcut_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(
            "Click a chord and press the keys you want. Bindings are stored in settings.json \
             and can also be created programmatically with Settings::bind.",
        );
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(
                egui::TextEdit::singleline(&mut self.shortcut_search)
                    .desired_width(220.0)
                    .hint_text("Command or chord"),
            );
            if ui.button("Restore defaults").clicked() {
                self.settings.reset_shortcuts();
                self.status = "Shortcut map restored.".to_owned();
            }
        });
        ui.separator();
        let capturing = self.capturing_shortcut;
        // Groups are collapsible: each header is a collapsing header holding its
        // own grid, so a long key map stays navigable.
        let filter = self.shortcut_search.to_ascii_lowercase();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut current_group = "";
                let mut group_actions: Vec<ShortcutAction> = Vec::new();
                let mut groups: Vec<(&'static str, Vec<ShortcutAction>)> = Vec::new();
                for action in ShortcutAction::all() {
                    let group = action.group();
                    if group != current_group {
                        if !current_group.is_empty() {
                            groups.push((current_group, std::mem::take(&mut group_actions)));
                        }
                        current_group = group;
                    }
                    group_actions.push(*action);
                }
                if !current_group.is_empty() {
                    groups.push((current_group, group_actions));
                }
                for (group, actions) in groups {
                    let visible: Vec<ShortcutAction> = actions
                        .into_iter()
                        .filter(|action| {
                            if filter.is_empty() {
                                return true;
                            }
                            let keys = self.settings.keys_for(*action);
                            action.label().to_ascii_lowercase().contains(&filter)
                                || keys.to_ascii_lowercase().contains(&filter)
                                || group.to_ascii_lowercase().contains(&filter)
                        })
                        .collect();
                    if visible.is_empty() {
                        continue;
                    }
                    let header = egui::RichText::new(format!("{group} ({})", visible.len()))
                        .strong()
                        .color(self.ui_accent());
                    egui::CollapsingHeader::new(header)
                        .id_salt(("shortcut_group", group))
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::Grid::new(("shortcut_grid", group))
                                .num_columns(3)
                                .striped(true)
                                .show(ui, |ui| {
                                    for action in visible {
                                        let keys = self.settings.keys_for(action);
                                        let label = if capturing == Some(action) {
                                            "Press keys?".to_owned()
                                        } else if keys.is_empty() {
                                            "Unbound".to_owned()
                                        } else {
                                            keys.to_owned()
                                        };
                                        ui.label(action.label());
                                        if ui
                                            .add_sized(
                                                [150.0, 20.0],
                                                egui::Button::new(label)
                                                    .selected(capturing == Some(action)),
                                            )
                                            .clicked()
                                        {
                                            self.capturing_shortcut = Some(action);
                                            self.shortcut_draft.clear();
                                        }
                                        if ui.small_button("Clear").clicked() {
                                            self.settings.unbind(action);
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                }
            });
        if capturing.is_some()
            && let Some(chord) = Self::capture_chord(ui.ctx())
        {
            if let Some(action) = self.capturing_shortcut.take() {
                let conflicts: Vec<ShortcutAction> = self
                    .settings
                    .actions_for_chord(&chord.display())
                    .into_iter()
                    .filter(|other| *other != action)
                    .collect();
                self.settings.bind(action, &chord.display());
                self.status = if conflicts.is_empty() {
                    format!("Bound {} to {}.", action.label(), chord.display())
                } else {
                    format!(
                        "Bound {} to {}; this also matches {}.",
                        action.label(),
                        chord.display(),
                        conflicts
                            .iter()
                            .map(|conflict| conflict.label())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
            }
        }
    }

    /// Reads the first key chord in the current frame, used by the capture UI.
    fn capture_chord(context: &egui::Context) -> Option<Chord> {
        context.input(|input| {
            let mut chord = Chord {
                ctrl: input.modifiers.ctrl,
                shift: input.modifiers.shift,
                alt: input.modifiers.alt,
                key: None,
            };
            for event in &input.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } = event
                {
                    let name = key.name();
                    if let Some(canonical) = settings::KEY_NAMES
                        .iter()
                        .find(|candidate| candidate.eq_ignore_ascii_case(name))
                    {
                        chord.key = Some(*canonical);
                        return Some(chord);
                    }
                }
            }
            None
        })
    }

    fn render_plugin_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("Folders searched for installed plug-ins. They appear in the plug-in picker.");
        Self::path_list_editor(ui, "VST 2", &mut self.settings.vst_paths);
        Self::path_list_editor(ui, "VST3", &mut self.settings.vst3_paths);
        Self::path_list_editor(ui, "CLAP", &mut self.settings.clap_paths);
        ui.separator();
        if ui.button("Rescan plug-ins").clicked() {
            self.rescan_plugins();
        }
        if let Some(error) = &self.plugin_scan_error {
            ui.colored_label(egui::Color32::from_rgb(228, 178, 92), error);
        }
        ui.label(format!(
            "{} plug-in(s) discovered",
            self.external_plugins.len()
        ));
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .show(ui, |ui| {
                for plugin in &self.external_plugins {
                    ui.label(format!("{}  ?  {}", plugin.label(), plugin.path));
                }
            });
        ui.separator();
        ui.label(
            "VST3 and CLAP bundles are listed here and can be inserted on any mixer slot or \
             channel. Inserts process audio in 64-frame blocks (about 1.3 ms of latency at \
             48 kHz) and generators accept notes and pitch bend from the sequencer. Opening a \
             hosted plug-in attaches its own editor window.",
        );
    }

    fn render_sample_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("Folders listed in the sample browser.");
        Self::path_list_editor(ui, "Samples", &mut self.settings.sample_paths);
        if ui.button("Rescan samples").clicked() {
            self.refresh_sample_files();
        }
        ui.label(format!("{} sample(s) found", self.sample_files.len()));
    }

    fn render_interface_settings(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.settings.animations,
            "Animate interface transitions",
        );
        ui.add(
            egui::Slider::new(&mut self.settings.animation_speed, 0.1..=4.0)
                .text("Animation speed"),
        );
        ui.separator();
        let mut smoothing = self.settings.parameter_smoothing;
        ui.checkbox(&mut smoothing, "Smooth parameter changes (linear ramp)");
        let ramp_changed = ui
            .add(
                egui::Slider::new(&mut self.settings.smoothing_ms, 0.0..=200.0)
                    .text("Ramp length (ms)"),
            )
            .changed();
        let toggled = smoothing != self.settings.parameter_smoothing;
        if toggled {
            self.settings.parameter_smoothing = smoothing;
        }
        if ramp_changed || toggled {
            self.apply_audio_settings();
        }
        ui.separator();
        ui.add(
            egui::Slider::new(&mut self.settings.default_track_height, 34.0..=200.0)
                .text("New track height"),
        );
        if ui.button("Apply height to all tracks").clicked() {
            let height = self.settings.default_track_height;
            self.push_history();
            for lane in &mut self.project.lanes {
                lane.height = height;
            }
            self.sync_project();
        }
        ui.add(
            egui::Slider::new(&mut self.settings.default_velocity, 1..=127)
                .text("New note velocity"),
        );
        ui.add(
            egui::Slider::new(&mut self.settings.metronome_volume, 0.0..=1.0)
                .text("Metronome level"),
        );
    }

    fn render_graphics_settings(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.settings.animations,
            "Animate interface transitions",
        );
        ui.checkbox(
            &mut self.settings.fancy_animations,
            "Fancy animations (bounce and overshoot)",
        );
        ui.add(
            egui::Slider::new(&mut self.settings.animation_speed, 0.1..=4.0)
                .text("Animation speed"),
        );
        ui.separator();
        ui.add(egui::Slider::new(&mut self.settings.zoom, MIN_ZOOM..=MAX_ZOOM).text("Arrangement zoom"));
        ui.add(
            egui::Slider::new(&mut self.settings.wheel_step_ticks, 1..=32)
                .text("Alt+wheel step (ticks)"),
        );
        ui.add(
            egui::Slider::new(&mut self.settings.ui_scale, 0.6..=2.5)
                .text("Interface scale")
                .suffix(" x"),
        );
        ui.label("Ctrl+wheel zooms the arrangement and editors; Alt+wheel changes the step size.");
        ui.label("Ctrl+click the ruler to reset the zoom; the interface scale works like a DPI setting.");
        ui.separator();
        ui.label("Tooltips");
        for mode in settings::TooltipMode::ALL {
            ui.radio_value(&mut self.settings.tooltips, mode, mode.label());
        }
        if self.settings.tooltips == settings::TooltipMode::Hover {
            ui.add(
                egui::Slider::new(&mut self.settings.tooltip_delay, 0.0..=3.0)
                    .text("Hover delay (s)"),
            );
        }
    }

    fn render_theming_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(
            "A theme only changes appearance: colours used by meters, clips, MIDI notes and \
             panels, plus the font scale and an optional background image.",
        );
        ui.horizontal(|ui| {
            ui.label("Preset");
            for preset in settings::Theme::presets() {
                if ui.button(&preset.name).clicked() {
                    self.settings.theme = preset;
                }
            }
        });
        ui.separator();
        egui::Grid::new("theme_colors")
            .num_columns(2)
            .show(ui, |ui| {
                let mut swatch = |label: &str, channels: &mut [u8; 3]| {
                    ui.label(label);
                    let mut color = egui::Color32::from_rgb(channels[0], channels[1], channels[2]);
                    if ui.color_edit_button_srgba(&mut color).changed() {
                        *channels = [color.r(), color.g(), color.b()];
                    }
                    ui.end_row();
                };
                let theme = &mut self.settings.theme;
                swatch("Accent", &mut theme.accent);
                swatch("Background", &mut theme.background);
                swatch("Panel", &mut theme.panel);
                swatch("MIDI note / clip", &mut theme.note);
                swatch("Meter low", &mut theme.meter_low);
                swatch("Meter mid", &mut theme.meter_mid);
                swatch("Meter high", &mut theme.meter_high);
                swatch("Text", &mut theme.text);
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Font scale");
            ui.add(
                egui::Slider::new(&mut self.settings.theme.font_scale, 0.75..=1.6)
                    .fixed_decimals(2),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Background image");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.theme.background_image)
                    .desired_width(280.0)
                    .hint_text("Path to a PNG/JPG used behind the arrangement"),
            );
            if ui.button("Clear").clicked() {
                self.settings.theme.background_image.clear();
            }
        });
        if ui.button("Reset theme").clicked() {
            self.settings.theme = settings::Theme::default();
        }
    }

    fn render_general_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("Placeholder page for general options.");
        ui.checkbox(
            &mut self.settings.restore_last_project,
            "Reopen the last project on startup",
        );
        ui.add(
            egui::Slider::new(&mut self.settings.autosave_minutes, 0..=120)
                .text("Autosave every (minutes, 0 = off)"),
        );
        ui.add(
            egui::Slider::new(&mut self.settings.metronome_volume, 0.0..=1.0)
                .text("Metronome level"),
        );
    }

    /// Shared editor for a list of directories.
    fn path_list_editor(ui: &mut egui::Ui, title: &str, paths: &mut Vec<String>) {
        ui.label(egui::RichText::new(title).strong());
        let mut remove = None;
        for (index, path) in paths.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(path)
                        .desired_width(320.0)
                        .hint_text("Folder path"),
                );
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            paths.remove(index);
        }
        if ui.button("Add folder").clicked() {
            paths.push(String::new());
        }
        ui.add_space(6.0);
    }

    /// Sample browser: a channel-rack-shaped list without the step sequencer.
    fn render_sample_browser(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.sample_tab == SampleTab::Builtin, "Built-in")
                .clicked()
            {
                self.sample_tab = SampleTab::Builtin;
            }
            if ui
                .selectable_label(self.sample_tab == SampleTab::Folders, "Folders")
                .clicked()
            {
                self.sample_tab = SampleTab::Folders;
                if self.sample_files.is_empty() {
                    self.refresh_sample_files();
                }
            }
            if ui.button("Rescan").clicked() {
                self.refresh_sample_files();
            }
            if ui.button("Settings").clicked() {
                self.windows.settings = true;
                self.settings_tab = SettingsTab::Samples;
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.sample_tab {
                SampleTab::Builtin => {
                    for kind in [
                        DefaultSample::Kick,
                        DefaultSample::Snare,
                        DefaultSample::HiHat,
                    ] {
                        ui.horizontal(|ui| {
                            if ui
                                .add_sized(
                                    [180.0, 22.0],
                                    egui::Button::new(default_sample_label(kind)),
                                )
                                .clicked()
                            {
                                self.load_default_sample_into_selected(kind);
                            }
                            ui.label("adds to the selected instrument");
                        });
                    }
                }
                SampleTab::Folders => {
                    if self.sample_files.is_empty() {
                        ui.label("No folders configured in Settings ? Samples.");
                    }
                    for path in self.sample_files.clone() {
                        ui.horizontal(|ui| {
                            let name = path
                                .rsplit(['/', '\\'])
                                .next()
                                .unwrap_or(path.as_str())
                                .to_owned();
                            if ui
                                .add_sized([220.0, 20.0], egui::Button::new(name))
                                .clicked()
                            {
                                self.preview_sample(&path);
                            }
                            if ui
                                .small_button("Load")
                                .on_hover_text("Load into the selected instrument")
                                .clicked()
                            {
                                self.load_sample_into_selected(&path);
                            }
                        });
                    }
                }
            });
    }

    /// Assigns a built-in drum sample to the selected instrument.
    fn load_default_sample_into_selected(&mut self, kind: DefaultSample) {
        let instrument_id = self
            .selected_instrument
            .min(self.project.instruments.len() - 1);
        self.push_history();
        let instrument = &mut self.project.instruments[instrument_id];
        instrument.default_sample = kind;
        instrument.sample_path.clear();
        instrument.name = default_sample_label(kind).to_owned();
        self.sync_project();
        self.load_instrument_samples();
        self.status = format!("{} loaded.", default_sample_label(kind));
    }

    fn load_sample_into_selected(&mut self, path: &str) {
        let instrument_id = self
            .selected_instrument
            .min(self.project.instruments.len() - 1);
        self.push_history();
        self.project.instruments[instrument_id].sample_path = path.to_owned();
        self.project.instruments[instrument_id].default_sample = DefaultSample::None;
        self.sync_project();
        self.load_instrument_sample(instrument_id);
    }
}

/// Static data cached per hosted plug-in, so the wrapper does not re-query the
/// bundle every frame.
struct HostedMeta {
    info: pulseforge_hosting::BundleInfo,
    parameters: Vec<pulseforge_hosting::ParameterInfo>,
    editor_size: (u32, u32),
}

struct Knob {
    label: &'static str,
    min: f32,
    max: f32,
    suffix: &'static str,
    size: f32,
    default: Option<f32>,
    automation_target: Option<ParameterTarget>,
}

impl Knob {
    fn new(label: &'static str, min: f32, max: f32, suffix: &'static str) -> Self {
        Self {
            label,
            min,
            max,
            suffix,
            size: 58.0,
            default: None,
            automation_target: None,
        }
    }

    fn with_default(mut self, default: f32) -> Self {
        self.default = Some(default);
        self
    }

    fn automates(mut self, target: ParameterTarget) -> Self {
        self.automation_target = Some(target);
        self
    }

    /// Compact knobs are used inside dense rows such as the channel rack.
    fn compact(mut self) -> Self {
        self.size = 22.0;
        self
    }

    fn show(self, ui: &mut egui::Ui, value: &mut f32) -> bool {
        let compact = self.size < 40.0;
        let height = if compact { 40.0 } else { 76.0 };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(self.size, height), egui::Sense::click_and_drag());
        let mut changed = false;
        // A knob drag is relative: hide the pointer while it runs and put it
        // back where it started so the value does not jump on release.
        let drag_id = response.id.with("knob_drag_origin");
        if response.drag_started() {
            ui.data_mut(|data| data.insert_temp(drag_id, platform::cursor_position()));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
        }
        if response.drag_stopped() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
            if let Some(Some((x, y))) = ui.data_mut(|data| data.remove_temp(drag_id)) {
                platform::set_cursor_position(x, y);
            }
        }
        if response.dragged() {
            let delta = ui.input(|input| input.pointer.delta().y);
            *value = (*value - delta * (self.max - self.min) / 140.0).clamp(self.min, self.max);
            changed = true;
        }
        let (alt, control) = ui.input(|input| (input.modifiers.alt, input.modifiers.ctrl));
        if response.clicked_by(egui::PointerButton::Primary) && alt {
            *value = self.default.unwrap_or((self.min + self.max) * 0.5);
            changed = true;
        }
        if response.clicked_by(egui::PointerButton::Primary) && control {
            ui.data_mut(|data| data.insert_temp(response.id.with("numeric_entry"), true));
        }
        let painter = ui.painter_at(rect);
        let center = egui::pos2(
            rect.center().x,
            rect.top() + if compact { 14.0 } else { 21.0 },
        );
        let radius = if compact { 9.5 } else { 15.0 };
        painter.circle_filled(center, radius + 3.0, egui::Color32::from_gray(18));
        painter.circle_filled(center, radius, egui::Color32::from_gray(51));
        painter.circle_stroke(
            center,
            radius,
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(115)),
        );

        let automated = self.automation_target.is_some_and(|target| {
            ui.data(|data| {
                data.get_temp::<Vec<ParameterTarget>>(egui::Id::new(AUTOMATED_TARGETS_KEY))
                    .is_some_and(|targets| targets.contains(&target))
            })
        });
        let live_value = self.automation_target.and_then(|target| {
            ui.data(|data| {
                data.get_temp::<Vec<(ParameterTarget, f32)>>(egui::Id::new(AUTOMATED_VALUES_KEY))
                    .and_then(|values| {
                        values
                            .iter()
                            .find(|(candidate, _)| *candidate == target)
                            .map(|(_, value)| *value)
                    })
            })
        });
        if automated {
            painter.circle_stroke(
                center,
                radius + 5.0,
                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(202, 158, 238)),
            );
        }

        let normalized = ((*value - self.min) / (self.max - self.min)).clamp(0.0, 1.0);
        let angle = (-135.0 + normalized * 270.0).to_radians();
        let indicator = center + egui::vec2(angle.sin(), -angle.cos()) * (radius - 3.0);
        painter.line_segment(
            [center, indicator],
            egui::Stroke::new(2.2_f32, egui::Color32::from_rgb(112, 208, 190)),
        );
        if let Some(live) = live_value {
            let live_normalized = ((live - self.min) / (self.max - self.min)).clamp(0.0, 1.0);
            let live_angle = (-135.0 + live_normalized * 270.0).to_radians();
            let live_indicator =
                center + egui::vec2(live_angle.sin(), -live_angle.cos()) * (radius + 1.0);
            painter.circle_filled(live_indicator, 2.6, egui::Color32::from_rgb(238, 205, 116));
        }
        let mut arc_points = Vec::with_capacity(19);
        for index in 0..=18 {
            let arc_angle = (-135.0 + index as f32 * 15.0).to_radians();
            arc_points
                .push(center + egui::vec2(arc_angle.sin(), -arc_angle.cos()) * (radius + 2.0));
        }
        painter.add(egui::Shape::line(
            arc_points,
            egui::Stroke::new(1.3_f32, egui::Color32::from_gray(88)),
        ));
        let label_y = if compact {
            rect.top() + 5.0
        } else {
            rect.top() + 45.0
        };
        // An empty label keeps the dial bare, which is what the mixer strips
        // want: the pan knob is identified by position, not by text.
        if !self.label.is_empty() {
            painter.text(
                egui::pos2(rect.center().x, label_y),
                egui::Align2::CENTER_CENTER,
                self.label,
                egui::FontId::proportional(if compact { 8.0 } else { 10.0 }),
                egui::Color32::from_gray(175),
            );
        }
        let shown = live_value.unwrap_or(*value);
        // Compact knobs omit the value text: their label sits above the dial and
        // the value stays available from the status bar and context menu.
        if !compact {
            painter.text(
                egui::pos2(rect.center().x, rect.top() + 62.0),
                egui::Align2::CENTER_CENTER,
                format!("{shown:.1}{}", self.suffix),
                egui::FontId::monospace(10.0),
                if live_value.is_some() {
                    egui::Color32::from_rgb(238, 205, 116)
                } else {
                    egui::Color32::WHITE
                },
            );
        }
        let numeric_id = response.id.with("numeric_entry");
        let mut editing = ui
            .data(|data| data.get_temp::<bool>(numeric_id))
            .unwrap_or(false);
        if editing {
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .range(self.min..=self.max)
                        .speed((self.max - self.min) / 100.0)
                        .suffix(self.suffix),
                )
                .changed();
            if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                ui.data_mut(|data| data.remove::<bool>(numeric_id));
                editing = false;
            }
        }
        response.context_menu(|ui| {
            ui.label(format!("{} value", self.label));
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .range(self.min..=self.max)
                        .speed((self.max - self.min) / 100.0)
                        .suffix(self.suffix),
                )
                .changed();
            if ui
                .add_enabled(
                    self.automation_target.is_some(),
                    egui::Button::new("Add automation lane..."),
                )
                .clicked()
            {
                if let Some(target) = self.automation_target {
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new("knob_add_automation"), target)
                    });
                    ui.close();
                }
            }
            if ui.button("Reset to default").clicked() {
                *value = self.default.unwrap_or((self.min + self.max) * 0.5);
                changed = true;
                ui.close();
            }
            ui.separator();
            ui.add_enabled(false, egui::Button::new("More parameter actions"));
        });
        response.on_hover_text(format!(
            "{}: {:.2}{} · Alt+click reset · Ctrl+click type · right-click menu",
            self.label, *value, self.suffix
        ));
        let _ = editing;
        changed
    }
}

/// What a power switch was asked to do, decided from the modifier keys at click
/// time.
use context_menu::PowerIntent;

/// Everything a power switch draws. One struct, one drawing path, so every enable
/// button in the app looks and behaves the same.
struct PowerSwitch<'a> {
    /// Name used by the tooltip.
    name: &'a str,
    enabled: bool,
    locked: bool,
    solo: bool,
    /// True when another switch of the same group is soloed (dims the badge).
    any_solo: bool,
    color: egui::Color32,
    size: egui::Vec2,
}

/// Paint and hit-test a power switch inside `rect`.
fn power_switch_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    switch: &PowerSwitch<'_>,
) -> PowerIntent {
    let response = ui.interact(rect, id, egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        3.0,
        if switch.enabled {
            switch.color.gamma_multiply(0.45)
        } else {
            egui::Color32::from_gray(38)
        },
    );
    paint_icon(
        &painter,
        rect.shrink(switch.size.x * 0.15),
        Icon::Power,
        if switch.enabled {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_gray(110)
        },
    );
    if switch.locked {
        painter.line_segment(
            [
                egui::pos2(rect.left() + 2.0, rect.bottom() - 2.0),
                egui::pos2(rect.right() - 2.0, rect.bottom() - 2.0),
            ],
            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(230, 178, 92)),
        );
    }
    if switch.solo || switch.any_solo {
        painter.circle_filled(
            egui::pos2(rect.right() - 3.0, rect.top() + 3.0),
            2.5,
            if switch.solo {
                egui::Color32::from_rgb(120, 200, 255)
            } else {
                egui::Color32::from_gray(70)
            },
        );
    }
    tooltip(
        ui,
        &response,
        &format!("{} · click powers, shift locks, ctrl solos", switch.name),
    );
    if !response.clicked() {
        return PowerIntent::default();
    }
    ui.input(|input| {
        if input.modifiers.shift {
            PowerIntent {
                lock: true,
                ..PowerIntent::default()
            }
        } else if input.modifiers.command || input.modifiers.ctrl {
            PowerIntent {
                solo: true,
                ..PowerIntent::default()
            }
        } else {
            PowerIntent {
                toggle: true,
                ..PowerIntent::default()
            }
        }
    })
}

/// Power switch laid out by the caller's `ui`, returning the requested intent.
fn power_switch(ui: &mut egui::Ui, switch: PowerSwitch<'_>) -> PowerIntent {
    let (rect, _) = ui.allocate_exact_size(switch.size, egui::Sense::hover());
    let id = ui.id().with(("power_switch", switch.name, rect.left() as i32, rect.top() as i32));
    power_switch_at(ui, rect, id, &switch)
}

struct MixerStrip<'a> {
    channel_id: usize,
    name: Option<&'a mut String>,
    pinned: Option<&'a mut bool>,
    gain: &'a mut f32,
    pan: &'a mut f32,
    /// Stereo width; absent on the master strip.
    width: Option<&'a mut f32>,
    eq: &'a mut ThreeBandEq,
    meter: f32,
    selected: bool,
    collapsed: bool,
    is_master: bool,
    enabled: bool,
    locked: bool,
    solo: bool,
    inserts: Option<&'a mut [InsertFxSlot]>,
    /// Channel FX bypass, absent on the master strip.
    fx_bypass: Option<&'a mut bool>,
    /// Insert slots whose plug-in window is currently open.
    open_inserts: &'a std::collections::BTreeSet<(usize, usize)>,
}

#[derive(Default)]
struct MixerStripResult {
    changed: bool,
    selected: bool,
    actions: Vec<ContextAction>,
}

/// Meter heights shared by the fader and the level meter so the pair reads as
/// one control.
const MIXER_FADER_HEIGHT: f32 = 132.0;
const MIXER_FADER_WIDTH: f32 = 30.0;

fn mixer_strip(ui: &mut egui::Ui, mut strip: MixerStrip<'_>) -> MixerStripResult {
    let mut result = MixerStripResult::default();
    let channel_color = if strip.is_master {
        egui::Color32::WHITE
    } else {
        lane_color(strip.channel_id)
    };
    let strip_width = if strip.collapsed { 96.0 } else { 206.0 };
    // A non-zero height keeps every strip in the horizontal row aligned at the
    // top instead of drifting down as each one grows.
    let strip_height = ui.available_height().max(120.0);
    // Any part of the strip selects the channel. The hit test uses the rect
    // recorded last frame so it can be registered *before* the widgets it
    // contains; a later registration would swallow their clicks.
    let bg_rect_id = egui::Id::new(("mixer_strip_rect", strip.channel_id));
    if let Some(rect) = ui.data(|data| data.get_temp::<egui::Rect>(bg_rect_id)) {
        let background = ui.interact(
            rect,
            egui::Id::new(("mixer_strip_hit", strip.channel_id)),
            egui::Sense::click(),
        );
        if background.clicked() {
            result.selected = true;
        }
        if background.secondary_clicked() {
            result.selected = true;
        }
    }
    // The strip owns a fixed width; allocating it up front stops the parent
    // horizontal layout from stretching it across the whole scroll area.
    let frame = ui
        .allocate_ui_with_layout(
            egui::vec2(strip_width, strip_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::group(ui.style())
                    .fill(egui::Color32::from_gray(29))
                    .stroke(egui::Stroke::new(
                        if strip.selected { 2.0_f32 } else { 1.0_f32 },
                        if strip.selected {
                            egui::Color32::WHITE
                        } else {
                            channel_color
                        },
                    ))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_min_width(strip_width - 16.0);
                        ui.set_max_width(strip_width - 16.0);

                        // Header: one power switch (shift locks, ctrl solos) plus the name,
                        // which is also the channel's selection target.
                        ui.horizontal(|ui| {
                            let power_size = egui::vec2(18.0, 18.0);
                            let (power_rect, power_response) =
                                ui.allocate_exact_size(power_size, egui::Sense::click());
                            let painter = ui.painter_at(power_rect);
                            painter.rect_filled(
                                power_rect,
                                3.0,
                                if strip.enabled {
                                    channel_color.gamma_multiply(0.45)
                                } else {
                                    egui::Color32::from_gray(38)
                                },
                            );
                            paint_icon(
                                &painter,
                                power_rect.shrink(2.5),
                                Icon::Power,
                                if strip.enabled {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::from_gray(110)
                                },
                            );
                            if strip.locked {
                                painter.line_segment(
                                    [
                                        egui::pos2(
                                            power_rect.left() + 2.0,
                                            power_rect.bottom() - 2.0,
                                        ),
                                        egui::pos2(
                                            power_rect.right() - 2.0,
                                            power_rect.bottom() - 2.0,
                                        ),
                                    ],
                                    egui::Stroke::new(
                                        2.0_f32,
                                        egui::Color32::from_rgb(230, 178, 92),
                                    ),
                                );
                            }
                            if strip.solo {
                                painter.circle_filled(
                                    egui::pos2(power_rect.right() - 3.0, power_rect.top() + 3.0),
                                    2.5,
                                    egui::Color32::from_rgb(120, 200, 255),
                                );
                            }
                            if power_response.clicked() {
                                result
                                    .actions
                                    .push(ContextAction::ToggleChannelEnabled(strip.channel_id));
                            }
                            tooltip(ui, &power_response, "Power - shift locks, ctrl solos");

                            let mut heading = if strip.is_master {
                                "MASTER".to_owned()
                            } else {
                                strip.name.as_deref().map_or_else(
                                    || format!("Insert {}", strip.channel_id + 1),
                                    |name| name.clone(),
                                )
                            };
                            if strip.collapsed && heading.chars().count() > 9 {
                                heading =
                                    format!("{}?", heading.chars().take(8).collect::<String>());
                            }
                            // The editable name already shows the channel, so
                            // no separate label is drawn beside it.
                            let editable =
                                !strip.is_master && strip.name.is_some() && !strip.collapsed;
                            let header = if editable {
                                ui.label(egui::RichText::new("").small())
                            } else {
                                let response = ui.selectable_label(
                                    strip.selected,
                                    egui::RichText::new(heading.clone())
                                        .color(channel_color)
                                        .strong(),
                                );
                                result.selected |= response.clicked();
                                response
                            };
                            if !strip.is_master {
                                let menu = ContextMenu {
                                    items: vec![
                                        MenuItem::Action {
                                            label: if strip
                                                .pinned
                                                .as_deref()
                                                .copied()
                                                .unwrap_or(false)
                                            {
                                                "Unpin channel"
                                            } else {
                                                "Pin channel"
                                            }
                                            .to_owned(),
                                            action: ContextAction::ToggleLanePin(strip.channel_id),
                                        },
                                        MenuItem::Action {
                                            label: if strip.enabled {
                                                "Disable channel"
                                            } else {
                                                "Enable channel"
                                            }
                                            .to_owned(),
                                            action: ContextAction::ToggleChannelEnabled(
                                                strip.channel_id,
                                            ),
                                        },
                                        MenuItem::Separator,
                                        MenuItem::Folder {
                                            label: "Routing".to_owned(),
                                            items: vec![MenuItem::Action {
                                                label: "Disconnect all cables".to_owned(),
                                                action: ContextAction::DisconnectChannel(
                                                    strip.channel_id,
                                                ),
                                            }],
                                        },
                                        MenuItem::Folder {
                                            label: "Channel".to_owned(),
                                            items: vec![MenuItem::Action {
                                                label: "Select channel".to_owned(),
                                                action: ContextAction::SelectChannel(
                                                    strip.channel_id,
                                                ),
                                            }],
                                        },
                                    ],
                                };
                                header.context_menu(|ui| {
                                    if let Some(action) = menu.show(ui) {
                                        result.actions.push(action);
                                    }
                                });
                                if let Some(name) = &mut strip.name
                                    && !strip.collapsed
                                    && ui
                                        .add(
                                            egui::TextEdit::singleline(&mut **name)
                                                .desired_width((strip_width - 60.0).max(60.0))
                                                .margin(egui::Margin::symmetric(4, 1)),
                                        )
                                        .changed()
                                {
                                    result.changed = true;
                                }
                            }
                        });

                        // Fader and meter: one centred pair with identical height.
                        let fader_height = if strip.collapsed {
                            66.0
                        } else {
                            MIXER_FADER_HEIGHT
                        };
                        let fader_width = if strip.collapsed {
                            20.0
                        } else {
                            MIXER_FADER_WIDTH
                        };
                        let pair_width = fader_width + 6.0 + 13.0;
                        let pad = ((strip_width - pair_width) * 0.5).max(0.0);
                        ui.horizontal(|ui| {
                            ui.add_space(pad);
                            if fader(ui, strip.gain, fader_width, fader_height) {
                                result.changed = true;
                            }
                            render_meter_sized(ui, strip.meter, 13.0, fader_height);
                        });
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new(fader_label(*strip.gain)).small());
                            ui.horizontal(|ui| {
                                result.changed |= Knob::new("", -1.0, 1.0, "")
                                    .compact()
                                    .show(ui, strip.pan);
                                if let Some(width) = strip.width.as_deref_mut() {
                                    result.changed |= Knob::new("", 0.0, 2.0, "")
                                        .with_default(1.0)
                                        .compact()
                                        .show(ui, width);
                                }
                            });
                        });

                        if !strip.collapsed
                            && let Some(inserts) = strip.inserts.as_deref_mut()
                        {
                            ui.separator();
                            if let Some(bypass) = strip.fx_bypass.as_deref_mut() {
                                // Dry/wet comparison that never touches the
                                // individual insert enable states.
                                if ui.selectable_label(*bypass, "FX bypass").clicked() {
                                    *bypass = !*bypass;
                                    result.changed = true;
                                }
                            }
                            let any_solo = inserts.iter().any(|slot| slot.solo && slot.enabled);
                            let rows: Vec<(usize, String, bool, bool, bool)> = inserts
                                .iter()
                                .enumerate()
                                .map(|(index, slot)| {
                                    (
                                        index,
                                        if slot.name.is_empty() {
                                            format!("Slot {}", index + 1)
                                        } else {
                                            slot.name.clone()
                                        },
                                        slot.enabled,
                                        slot.solo,
                                        slot.locked,
                                    )
                                })
                                .collect();
                            // Ten slots can be taller than the strip, so the
                            // list scrolls and every insert stays reachable.
                            egui::ScrollArea::vertical()
                                .id_salt(("mixer_inserts", strip.channel_id))
                                .max_height(150.0)
                                // A fixed height keeps every strip the same size
                                // whether or not it carries inserts.
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                            for (index, label, enabled, solo, locked) in rows {
                                let open = strip.open_inserts.contains(&(strip.channel_id, index));
                                ui.horizontal(|ui| {
                                    let intent = power_switch(
                                        ui,
                                        PowerSwitch {
                                            name: &label,
                                            enabled,
                                            locked,
                                            solo,
                                            any_solo,
                                            color: channel_color,
                                            size: egui::vec2(16.0, 16.0),
                                        },
                                    );
                                    if intent.any() {
                                        result.actions.push(ContextAction::ToggleInsertEnabled {
                                            channel: strip.channel_id,
                                            slot: index,
                                            intent,
                                        });
                                    }

                                    let text =
                                        egui::RichText::new(label).small().color(if enabled {
                                            egui::Color32::from_gray(225)
                                        } else {
                                            egui::Color32::from_gray(130)
                                        });
                                    let slot_button = ui.add_sized(
                                        [ui.available_width().max(60.0), 16.0],
                                        egui::Button::new(text)
                                            .fill(if open {
                                                egui::Color32::from_gray(62)
                                            } else {
                                                egui::Color32::from_gray(38)
                                            })
                                            .sense(egui::Sense::click()),
                                    );
                                    if slot_button.clicked() {
                                        result.actions.push(ContextAction::OpenInsertWindow {
                                            channel: strip.channel_id,
                                            slot: index,
                                        });
                                    }
                                    if slot_button.double_clicked() {
                                        result.actions.push(ContextAction::OpenInsertPicker {
                                            channel: strip.channel_id,
                                            slot: index,
                                        });
                                    }
                                    tooltip(
                                        ui,
                                        &slot_button,
                                        "Click: open - double click or middle click: change effect",
                                    );
                                    slot_button.context_menu(|ui| {
                                        let menu = ContextMenu {
                                            items: vec![
                                                MenuItem::Action {
                                                    label: "Change effect".to_owned(),
                                                    action: ContextAction::OpenInsertPicker {
                                                        channel: strip.channel_id,
                                                        slot: index,
                                                    },
                                                },
                                                MenuItem::Action {
                                                    label: "Open window".to_owned(),
                                                    action: ContextAction::OpenInsertWindow {
                                                        channel: strip.channel_id,
                                                        slot: index,
                                                    },
                                                },
                                                MenuItem::Separator,
                                                MenuItem::Action {
                                                    label: "Clear insert".to_owned(),
                                                    action: ContextAction::ClearInsert {
                                                        channel: strip.channel_id,
                                                        slot: index,
                                                    },
                                                },
                                            ],
                                        };
                                        if let Some(action) = menu.show(ui) {
                                            result.actions.push(action);
                                        }
                                    });
                                });
                            }
                                });
                            ui.label(
                                egui::RichText::new("Middle click changes an insert")
                                    .small()
                                    .color(egui::Color32::from_gray(120)),
                            );
                        }

                        if !strip.collapsed {
                            ui.separator();
                            ui.label(egui::RichText::new("3-BAND EQ").strong());
                            draw_eq_curve(ui, *strip.eq);
                            ui.horizontal(|ui| {
                                result.changed |= Knob::new("LOW", -12.0, 12.0, " dB")
                                    .show(ui, &mut strip.eq.low_db);
                                result.changed |= Knob::new("MID", -12.0, 12.0, " dB")
                                    .show(ui, &mut strip.eq.mid_db);
                                result.changed |= Knob::new("HIGH", -12.0, 12.0, " dB")
                                    .show(ui, &mut strip.eq.high_db);
                            });
                        }
                        if strip.pinned.as_deref().copied().unwrap_or(false) {
                            ui.label(egui::RichText::new("PINNED").small().color(channel_color));
                        }
                    });
            },
        )
        .response;
    ui.data_mut(|data| data.insert_temp(bg_rect_id, frame.rect));
    result
}

fn edit_time_value(
    ui: &mut egui::Ui,
    track_id: usize,
    label: &str,
    value: &mut project::TimeValue,
) -> bool {
    let mut changed = false;
    // Value and unit share one row so the control does not wrap onto two lines.
    ui.horizontal(|ui| {
        ui.label(label);
        changed |= ui
            .add(
                egui::DragValue::new(&mut value.amount)
                    .range(0.0..=60_000.0)
                    .speed(1.0),
            )
            .changed();
        egui::ComboBox::from_id_salt(("time_unit", track_id, label))
            .selected_text(match value.unit {
                project::TimeUnit::Steps => "steps",
                project::TimeUnit::Milliseconds => "ms",
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut value.unit, project::TimeUnit::Steps, "steps")
                    .changed();
                changed |= ui
                    .selectable_value(&mut value.unit, project::TimeUnit::Milliseconds, "ms")
                    .changed();
            });
    });
    changed
}

fn render_effect_parameters(
    ui: &mut egui::Ui,
    slot: &mut InsertFxSlot,
    channel_id: usize,
    slot_id: usize,
) -> bool {
    let mut changed = false;
    if slot.enabled && !slot.name.is_empty() {
        ui.horizontal(|ui| {
            changed |= Knob::new("MIX", 0.0, 1.0, "")
                .with_default(0.5)
                .automates(ParameterTarget::InsertEffect {
                    channel_id,
                    slot_id,
                    parameter: EffectParameter::Mix,
                })
                .show(ui, &mut slot.mix);
            match slot.name.as_str() {
                "3-Band EQ" => {
                    changed |= Knob::new("LOW", -12.0, 12.0, " dB")
                        .with_default(0.0)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::LowDb,
                        })
                        .show(ui, &mut slot.eq.low_db);
                    changed |= Knob::new("MID", -12.0, 12.0, " dB")
                        .with_default(0.0)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::MidDb,
                        })
                        .show(ui, &mut slot.eq.mid_db);
                    changed |= Knob::new("HIGH", -12.0, 12.0, " dB")
                        .with_default(0.0)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::HighDb,
                        })
                        .show(ui, &mut slot.eq.high_db);
                }
                "Reverb" | "Plate Reverb" => {
                    changed |= Knob::new("DECAY", 0.0, 1.0, "")
                        .with_default(0.35)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::Amount,
                        })
                        .show(ui, &mut slot.amount);
                    changed |= Knob::new("ROOM", 0.0, 1.0, "")
                        .with_default(0.55)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::RoomSize,
                        })
                        .show(ui, &mut slot.room_size);
                }
                "Delay" | "Tape Echo" => {
                    changed |= Knob::new("WET", 0.0, 1.0, "")
                        .with_default(0.35)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::Amount,
                        })
                        .show(ui, &mut slot.amount);
                    egui::ComboBox::from_id_salt(("effect_time_unit", channel_id, slot_id))
                        .selected_text(match slot.time_unit {
                            project::TimeUnit::Steps => "Steps",
                            project::TimeUnit::Milliseconds => "ms",
                        })
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(
                                    &mut slot.time_unit,
                                    project::TimeUnit::Steps,
                                    "Steps",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut slot.time_unit,
                                    project::TimeUnit::Milliseconds,
                                    "Milliseconds",
                                )
                                .changed();
                        });
                    let mut time_amount = slot.time_amount();
                    match slot.time_unit {
                        project::TimeUnit::Steps => {
                            changed |= Knob::new("TIME", 0.01, 16.0, " steps")
                                .with_default(1.0)
                                .automates(ParameterTarget::InsertEffect {
                                    channel_id,
                                    slot_id,
                                    parameter: EffectParameter::TimeSeconds,
                                })
                                .show(ui, &mut time_amount);
                        }
                        project::TimeUnit::Milliseconds => {
                            changed |= Knob::new("TIME", 10.0, 2_000.0, " ms")
                                .with_default(320.0)
                                .automates(ParameterTarget::InsertEffect {
                                    channel_id,
                                    slot_id,
                                    parameter: EffectParameter::TimeSeconds,
                                })
                                .show(ui, &mut time_amount);
                        }
                    }
                    slot.set_time_amount(time_amount);
                    changed |= Knob::new("FEEDBACK", 0.0, 0.94, "")
                        .with_default(0.35)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::Feedback,
                        })
                        .show(ui, &mut slot.feedback);
                }
                "Juno Chorus" | "Phaser" => {
                    changed |= Knob::new("AMOUNT", 0.0, 1.0, "")
                        .with_default(0.35)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::Amount,
                        })
                        .show(ui, &mut slot.amount);
                    changed |= Knob::new("RATE", 0.05, 8.0, " Hz")
                        .with_default(0.8)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::RateHz,
                        })
                        .show(ui, &mut slot.rate_hz);
                    changed |= Knob::new("DEPTH", 0.0, 1.0, "")
                        .with_default(0.5)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::Depth,
                        })
                        .show(ui, &mut slot.depth);
                }
                "Waveshaper" => {
                    changed |= Knob::new("DRIVE", 0.0, 1.0, "")
                        .with_default(0.5)
                        .automates(ParameterTarget::InsertEffect {
                            channel_id,
                            slot_id,
                            parameter: EffectParameter::Drive,
                        })
                        .show(ui, &mut slot.drive);
                }
                _ => {}
            }
        });
    }
    changed
}

/// Vertical dB fader drawn to exactly `height`, so its travel matches the meter
/// beside it. [`fader_position`]/[`fader_gain`] provide the dB scale and the
/// silence snap at the bottom of the travel.
fn fader(ui: &mut egui::Ui, gain: &mut f32, width: f32, height: f32) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(22));
    let rail = egui::Rect::from_center_size(
        rect.center(),
        egui::vec2(6.0, (rect.height() - 10.0).max(10.0)),
    );
    painter.rect_filled(rail, 3.0, egui::Color32::from_gray(52));
    let mut changed = false;
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let position = ((rail.bottom() - pointer.y) / rail.height()).clamp(0.0, 1.0);
        *gain = fader_gain(position);
        changed = true;
    }
    let position = fader_position(*gain);
    let level_y = rail.bottom() - position * rail.height();
    let accent = egui::Color32::from_rgb(112, 208, 190);
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(rail.left(), level_y), rail.right_bottom()),
        3.0,
        accent.gamma_multiply(0.7),
    );
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(rect.center().x, level_y),
            egui::vec2((width - 4.0).max(8.0), 9.0),
        ),
        2.0,
        egui::Color32::from_gray(205),
    );
    tooltip(ui, &response, "Volume · dB scale, bottom = silence");
    changed
}

fn render_meter_sized(ui: &mut egui::Ui, level: f32, width: f32, height: f32) {
    let theme = active_theme(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme.color(theme.panel).gamma_multiply(0.5));
    let segments = (height / 5.0).floor().max(8.0) as usize;
    let active_segments = (level.clamp(0.0, 1.0) * segments as f32).ceil() as usize;
    for segment in 0..active_segments {
        let fraction = segment as f32 / segments as f32;
        let color = if fraction > 0.88 {
            theme.color(theme.meter_high)
        } else if fraction > 0.72 {
            theme.color(theme.meter_mid)
        } else {
            theme.color(theme.meter_low)
        };
        let y = rect.bottom() - (segment + 1) as f32 * (rect.height() / segments as f32);
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rect.left() + 2.0, y + 1.0),
                egui::vec2(rect.width() - 4.0, rect.height() / segments as f32 - 2.0),
            ),
            1.0,
            color,
        );
    }
}

fn draw_eq_curve(ui: &mut egui::Ui, eq: ThreeBandEq) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(192.0, 82.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(18));
    let center_y = rect.center().y;
    painter.line_segment(
        [
            egui::pos2(rect.left() + 5.0, center_y),
            egui::pos2(rect.right() - 5.0, center_y),
        ],
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(66)),
    );
    let points: Vec<_> = (0..=64)
        .map(|index| {
            let x = index as f32 / 64.0;
            let low = eq.low_db * (-((x - 0.16) / 0.24).powi(2)).exp();
            let mid = eq.mid_db * (-((x - 0.50) / 0.20).powi(2)).exp();
            let high = eq.high_db * (-((x - 0.84) / 0.24).powi(2)).exp();
            let db = (low + mid + high).clamp(-12.0, 12.0);
            egui::pos2(
                rect.left() + x * rect.width(),
                center_y - db / 12.0 * (rect.height() * 0.42),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(112, 208, 190)),
    ));
}

/// Pushes a [`settings::Theme`] into egui's visuals and typography, and stores
/// it in context data so free drawing helpers can read the same colours.
fn apply_theme(context: &egui::Context, theme: &settings::Theme) {
    let panel = theme.color(theme.panel);
    let background = theme.color(theme.background);
    let accent = theme.color(theme.accent);
    let text = theme.color(theme.text);
    let mut visuals = if theme.background.iter().all(|channel| *channel > 128) {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.override_text_color = Some(text);
    visuals.window_fill = panel;
    visuals.panel_fill = background;
    visuals.extreme_bg_color = panel.gamma_multiply(0.55);
    visuals.faint_bg_color = panel.gamma_multiply(1.25);
    visuals.selection.bg_fill = accent.gamma_multiply(0.45);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, accent);
    visuals.widgets.active.bg_fill = accent.gamma_multiply(0.5);
    visuals.widgets.hovered.bg_fill = panel.gamma_multiply(1.35);
    visuals.widgets.inactive.bg_fill = panel.gamma_multiply(1.1);
    visuals.widgets.noninteractive.bg_fill = panel;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, panel.gamma_multiply(1.6));
    context.set_visuals(visuals);
    let scale = theme.font_scale.clamp(0.75, 1.6);
    context.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(7.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.visuals.window_corner_radius = egui::CornerRadius::same(8);
        for font in style.text_styles.values_mut() {
            font.size = (font.size * scale).max(6.0);
        }
    });
    context.data_mut(|data| {
        data.insert_temp(egui::Id::new(THEME_KEY), theme.clone());
    });
}

/// Context-data key holding the active theme.
const THEME_KEY: &str = "active_theme";

/// Active theme for free drawing helpers.
fn active_theme(context: &egui::Context) -> settings::Theme {
    context
        .data(|data| data.get_temp::<settings::Theme>(egui::Id::new(THEME_KEY)))
        .unwrap_or_default()
}

fn lane_color(index: usize) -> egui::Color32 {
    const COLORS: [egui::Color32; 8] = [
        egui::Color32::from_rgb(211, 115, 91),
        egui::Color32::from_rgb(209, 163, 77),
        egui::Color32::from_rgb(92, 178, 133),
        egui::Color32::from_rgb(91, 157, 208),
        egui::Color32::from_rgb(154, 119, 205),
        egui::Color32::from_rgb(198, 112, 167),
        egui::Color32::from_rgb(88, 184, 181),
        egui::Color32::from_rgb(176, 175, 91),
    ];
    COLORS[index % COLORS.len()]
}

fn color_lerp(start: egui::Color32, end: egui::Color32, amount: f32) -> egui::Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |left: u8, right: u8| {
        (f32::from(left) + (f32::from(right) - f32::from(left)) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    egui::Color32::from_rgb(
        channel(start.r(), end.r()),
        channel(start.g(), end.g()),
        channel(start.b(), end.b()),
    )
}

fn point_segment_distance(point: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let amount = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * amount)
}

fn rack_step_at(
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

fn rack_step_span(start: u64, end: u64) -> (u64, u64) {
    (start.min(end), start.max(end))
}

fn clip_sources_match(left: &ClipSource, right: &ClipSource) -> bool {
    match (left, right) {
        (
            ClipSource::Pattern {
                pattern_id: left_id,
            },
            ClipSource::Pattern {
                pattern_id: right_id,
            },
        ) => left_id == right_id,
        (ClipSource::Audio { path: left_path }, ClipSource::Audio { path: right_path }) => {
            left_path == right_path
        }
        _ => false,
    }
}

/// Shared editing-tool strip so the piano roll and the clip arrangement expose
/// the same tools. The arrangement omits Join, which only merges notes.
fn tool_palette(ui: &mut egui::Ui, tool: &mut PianoTool, include_join: bool) {
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
fn tool_cursor_kind(tool: PianoTool, hovering_edge: bool) -> &'static str {
    match (tool, hovering_edge) {
        (_, true) => "resize",
        (PianoTool::Pen, false) => "pen",
        (PianoTool::Select, false) => "move",
        (PianoTool::Brush, false) => "brush",
        (PianoTool::Join, false) => "join",
        (PianoTool::Cut, false) => "cut",
    }
}

/// Vector toolbar icons.
///
/// These are painted with shapes rather than font glyphs because the bundled
/// fonts do not cover symbols such as U+23FB or U+1F512, which rendered as
/// empty boxes. Shapes also stay crisp at any DPI.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Icon {
    Rack,
    Roll,
    Mixer,
    Routing,
    Automation,
    Samples,
    Midi,
    Settings,
    Render,
    Debug,
    Undo,
    Redo,
    Play,
    Stop,
    Loop,
    Metronome,
    Power,
    Solo,
    ArrowDown,
    ArrowRight,
}

/// Draws `icon` inside `rect` using strokes only.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, icon: Icon, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.4_f32, color);
    let mid = rect.center();
    let size = rect.width().min(rect.height());
    let unit = size * 0.5;
    let at = |x: f32, y: f32| egui::pos2(mid.x + x * unit, mid.y + y * unit);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    match icon {
        Icon::Rack => {
            for row in 0..3 {
                let y = -0.6 + row as f32 * 0.6;
                line((-0.7, y), (0.7, y));
                painter.circle_filled(at(-0.45 + row as f32 * 0.25, y), unit * 0.12, color);
            }
        }
        Icon::Roll => {
            for step in 0..3 {
                let x = -0.6 + step as f32 * 0.6;
                line((x, -0.7), (x, 0.7));
            }
            for step in 0..3 {
                let y = -0.6 + step as f32 * 0.6;
                line((-0.7, y), (0.7, y));
            }
        }
        Icon::Mixer => {
            for (index, height) in [0.6_f32, 0.1, 0.35].into_iter().enumerate() {
                let x = -0.5 + index as f32 * 0.5;
                line((x, -0.7), (x, 0.7));
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        at(x, -0.7 + height),
                        egui::vec2(unit * 0.5, unit * 0.24),
                    ),
                    1.0,
                    color,
                );
            }
        }
        Icon::Routing => {
            painter.circle_stroke(at(-0.55, 0.45), unit * 0.22, stroke);
            painter.circle_stroke(at(0.55, -0.45), unit * 0.22, stroke);
            line((-0.35, 0.3), (0.35, -0.3));
        }
        Icon::Automation => {
            let points = [
                at(-0.7, 0.55),
                at(-0.2, 0.05),
                at(0.25, 0.3),
                at(0.7, -0.55),
            ];
            for pair in points.windows(2) {
                painter.line_segment([pair[0], pair[1]], stroke);
            }
            for point in points {
                painter.circle_filled(point, unit * 0.1, color);
            }
        }
        Icon::Samples => {
            line((-0.7, 0.0), (0.7, 0.0));
            for index in 0..7 {
                let x = -0.6 + index as f32 * 0.2;
                let amplitude = if index % 2 == 0 { 0.5 } else { 0.22 };
                line((x, -amplitude), (x, amplitude));
            }
        }
        Icon::Midi => {
            painter.rect_stroke(
                egui::Rect::from_center_size(mid, egui::vec2(unit * 1.4, unit * 1.0)),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            for index in 0..3 {
                let x = -0.35 + index as f32 * 0.35;
                painter.circle_filled(at(x, 0.0), unit * 0.09, color);
            }
        }
        Icon::Settings => {
            painter.circle_stroke(mid, unit * 0.5, stroke);
            for index in 0..6 {
                let angle = index as f32 * std::f32::consts::TAU / 6.0;
                let (sin, cos) = angle.sin_cos();
                line((cos * 0.55, sin * 0.55), (cos * 0.85, sin * 0.85));
            }
            painter.circle_filled(mid, unit * 0.14, color);
        }
        Icon::Render => {
            line((0.0, -0.7), (0.0, 0.25));
            line((-0.35, -0.05), (0.0, 0.3));
            line((0.35, -0.05), (0.0, 0.3));
            line((-0.7, 0.6), (0.7, 0.6));
        }
        Icon::Debug => {
            // Oscilloscope inside a frame: a "profile trace" glyph.
            painter.rect_stroke(
                egui::Rect::from_center_size(mid, egui::vec2(unit * 1.5, unit * 1.2)),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            let points = [at(-0.55, 0.1), at(-0.2, -0.4), at(0.1, 0.4), at(0.5, -0.25)];
            for pair in points.windows(2) {
                painter.line_segment([pair[0], pair[1]], stroke);
            }
        }
        Icon::Undo | Icon::Redo => {
            let direction = if icon == Icon::Undo { -1.0 } else { 1.0 };
            line((0.55 * direction, -0.5), (0.0, -0.5));
            line((0.0, -0.5), (0.0, 0.45));
            line((-0.55 * direction, 0.45), (0.55 * direction, 0.45));
            line((-0.55 * direction, 0.45), (-0.55 * direction, -0.5));
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![at(-0.45, -0.6), at(-0.45, 0.6), at(0.6, 0.0)],
                color,
                egui::Stroke::NONE,
            ));
        }
        Icon::Stop => {
            painter.rect_filled(
                egui::Rect::from_center_size(mid, egui::vec2(unit * 1.1, unit * 1.1)),
                1.5,
                color,
            );
        }
        Icon::Loop => {
            painter.circle_stroke(mid, unit * 0.6, stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![at(0.35, -0.85), at(0.85, -0.55), at(0.35, -0.3)],
                color,
                egui::Stroke::NONE,
            ));
        }
        Icon::Metronome => {
            line((-0.5, 0.7), (-0.15, -0.7));
            line((0.5, 0.7), (0.15, -0.7));
            line((-0.5, 0.7), (0.5, 0.7));
            line((0.0, 0.2), (0.4, -0.6));
        }
        Icon::Power => {
            painter.circle_stroke(mid, unit * 0.72, stroke);
            line((0.0, -0.95), (0.0, -0.15));
        }
        Icon::Solo => {
            painter.circle_stroke(mid, unit * 0.55, stroke);
            painter.circle_filled(mid, unit * 0.18, color);
        }
        Icon::ArrowDown => {
            line((-0.35, -0.2), (0.0, 0.3));
            line((0.35, -0.2), (0.0, 0.3));
        }
        Icon::ArrowRight => {
            line((-0.2, -0.35), (0.3, 0.0));
            line((-0.2, 0.35), (0.3, 0.0));
        }
    }
}

/// Square icon button. Icons are painted, so no font glyph coverage is needed.
fn icon_button(ui: &mut egui::Ui, icon: Icon, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
    if response.hovered() {
        ui.painter()
            .rect_stroke(rect, 3.0, visuals.bg_stroke, egui::StrokeKind::Inside);
    }
    let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
    paint_icon(ui.painter(), inner, icon, visuals.fg_stroke.color);
    tooltip(ui, &response, hint);
    response
}

/// Icon button that also shows an on/off state.
///
/// Used by the transport switches so a lit button means "on" without having to
/// read the status line.
fn icon_button_toggle(
    ui: &mut egui::Ui,
    icon: Icon,
    hint: &str,
    active: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    let fill = if active {
        ui.visuals().selection.bg_fill
    } else {
        visuals.weak_bg_fill
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    if active || response.hovered() {
        ui.painter()
            .rect_stroke(rect, 3.0, visuals.bg_stroke, egui::StrokeKind::Inside);
    }
    let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
    paint_icon(
        ui.painter(),
        inner,
        icon,
        if active {
            egui::Color32::WHITE
        } else {
            visuals.fg_stroke.color
        },
    );
    tooltip(ui, &response, hint);
    response
}

/// Word used by status messages for a power switch.
fn power_word(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

/// Folder that holds saved mixer-chain presets, one JSON file per preset.
const PRESET_DIR: &str = "presets/mixer";

/// Turns a user-typed preset name into a file stem that is safe on every
/// platform: letters, digits, spaces, dashes and underscores survive, everything
/// else becomes an underscore.
fn preset_file_stem(name: &str) -> String {
    let mut stem: String = name
        .trim()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    stem = stem.trim().to_owned();
    if stem.is_empty() {
        stem = "Preset".to_owned();
    }
    stem
}

/// Writes `inserts` to `presets/mixer/<name>.json` and returns the preset name.
fn save_chain_preset(name: &str, inserts: &[InsertFxSlot]) -> Result<String, String> {
    let stem = preset_file_stem(name);
    std::fs::create_dir_all(PRESET_DIR)
        .map_err(|error| format!("Could not create {PRESET_DIR}: {error}"))?;
    let json = serde_json::to_string_pretty(inserts)
        .map_err(|error| format!("Could not encode preset: {error}"))?;
    let path = format!("{PRESET_DIR}/{stem}.json");
    std::fs::write(&path, json).map_err(|error| format!("Could not write {path}: {error}"))?;
    Ok(stem)
}

/// Every saved preset name, sorted alphabetically.
fn list_chain_presets() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(PRESET_DIR) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("json"))
                .then(|| path.file_stem()?.to_str().map(ToOwned::to_owned))
                .flatten()
        })
        .collect();
    names.sort();
    names
}

/// Reads one preset back. Missing slots are simply not applied, so an older
/// preset still fits a channel with more inserts.
fn load_chain_preset(name: &str) -> Result<Vec<InsertFxSlot>, String> {
    let path = format!("{PRESET_DIR}/{}.json", preset_file_stem(name));
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("Could not read {path}: {error}"))?;
    serde_json::from_str::<Vec<InsertFxSlot>>(&text)
        .map_err(|error| format!("Invalid preset {path}: {error}"))
}

/// Applies the configured tooltip policy. Hover text is suppressed in the
/// default status-bar mode, where details are pushed to the status line.
fn tooltip(ui: &egui::Ui, response: &egui::Response, text: &str) {
    let mode =
        ui.data(|data| data.get_temp::<settings::TooltipMode>(egui::Id::new(TOOLTIP_MODE_KEY)));
    let delay = ui
        .data(|data| data.get_temp::<f32>(egui::Id::new(TOOLTIP_DELAY_KEY)))
        .unwrap_or(0.6);
    if response.hovered() {
        ui.data_mut(|data| {
            data.insert_temp(egui::Id::new(HOVER_TEXT_KEY), text.to_owned());
        });
        let id = response.id.with("tooltip_timer");
        let now = ui.input(|input| input.time);
        if ui.data(|data| data.get_temp::<f64>(id)).is_none() {
            ui.data_mut(|data| data.insert_temp(id, now));
        }
    } else {
        ui.data_mut(|data| {
            data.remove::<f64>(response.id.with("tooltip_timer"));
        });
    }
    match mode.unwrap_or_default() {
        settings::TooltipMode::StatusBar => {}
        settings::TooltipMode::Hover => {
            // egui has no delayed hover text, so the delay is tracked per widget.
            if response.hovered() {
                let id = response.id.with("tooltip_timer");
                let elapsed = ui
                    .data(|data| data.get_temp::<f64>(id))
                    .map_or(0.0, |started| ui.input(|input| input.time) - started);
                if elapsed >= f64::from(delay) {
                    response.clone().on_hover_text(text);
                }
            }
        }
        settings::TooltipMode::Immediate => {
            response.clone().on_hover_text(text);
        }
    }
}

/// Short clip label from a path.
fn clip_label(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("audio clip")
}

fn snap_label(snap_ticks: u64) -> &'static str {
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

fn snap_arrangement_step(step: u64, snap_ticks: u64) -> u64 {
    let snap_steps = snap_ticks.div_ceil(project::TICKS_PER_STEP).max(1);
    step.saturating_add(snap_steps / 2) / snap_steps * snap_steps
}

fn piano_snap_tick(tick: u64, snap_ticks: u64, alt: bool) -> u64 {
    if alt {
        tick
    } else {
        let snap_ticks = snap_ticks.max(1);
        tick.saturating_add(snap_ticks / 2) / snap_ticks * snap_ticks
    }
}

fn resize_note_start(
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

fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.00001 {
        FADER_MIN_DB
    } else {
        20.0 * gain.log10()
    }
}

/// Lowest level a mixer fader reaches before it snaps to silence, in dB.
const FADER_MIN_DB: f32 = -100.0;

/// Fader travel position (0 = silence, 1 = unity gain) for a linear gain.
fn fader_position(gain: f32) -> f32 {
    if gain <= 0.0 {
        0.0
    } else {
        (gain_to_db(gain) / FADER_MIN_DB).clamp(0.0, 1.0)
    }
}

/// Linear gain for a fader position. The bottom of the travel is exactly zero,
/// which is the "snap to 0" step at the end of the dB scale.
fn fader_gain(position: f32) -> f32 {
    let position = position.clamp(0.0, 1.0);
    if position <= 0.0 {
        0.0
    } else {
        10f32.powf(position * FADER_MIN_DB / 20.0)
    }
}

/// Fader read-out: finite dB values, and the infinity sign for silence.
fn fader_label(gain: f32) -> String {
    if gain <= 0.0 {
        "-\u{221E} dB".to_owned()
    } else {
        format!("{:.1} dB", gain_to_db(gain))
    }
}

fn instrument_parameter_options(kind: SynthKind) -> Vec<InstrumentParameter> {
    let mut parameters = vec![InstrumentParameter::Gain, InstrumentParameter::Pan];
    match kind {
        SynthKind::Ms20 => parameters.extend([
            InstrumentParameter::CutoffHz,
            InstrumentParameter::Resonance,
        ]),
        SynthKind::JunoPoly => parameters.extend([
            InstrumentParameter::CutoffHz,
            InstrumentParameter::Resonance,
            InstrumentParameter::PulseWidth,
            InstrumentParameter::SubLevel,
        ]),
        SynthKind::M1Workstation => {
            parameters.extend([InstrumentParameter::Tone, InstrumentParameter::Decay])
        }
        SynthKind::FmSynth => {
            parameters.extend([InstrumentParameter::FmRatio, InstrumentParameter::FmIndex])
        }
        SynthKind::Sine
        | SynthKind::SamplePlayer
        | SynthKind::SignalGenerator
        | SynthKind::External => {}
    }
    parameters
}

fn synth_kind_label(kind: SynthKind) -> &'static str {
    match kind {
        SynthKind::External => "External plug-in",
        SynthKind::SamplePlayer => "Sample Player",
        SynthKind::Sine => "Sine Synth",
        SynthKind::SignalGenerator => "Signal Generator",
        SynthKind::Ms20 => "MS-20 Mono",
        SynthKind::JunoPoly => "Juno-style Poly",
        SynthKind::M1Workstation => "M1-style Workstation",
        SynthKind::FmSynth => "2-Operator FM",
    }
}

fn waveform_label(waveform: SignalWaveform) -> &'static str {
    match waveform {
        SignalWaveform::Sine => "Sine",
        SignalWaveform::Triangle => "Triangle",
        SignalWaveform::Saw => "Saw",
        SignalWaveform::Square => "Square",
    }
}

fn default_sample_label(sample: DefaultSample) -> &'static str {
    match sample {
        DefaultSample::None => "None",
        DefaultSample::Kick => "Kick",
        DefaultSample::Snare => "Snare",
        DefaultSample::HiHat => "Hi-hat",
    }
}

fn effect_parameter_options(name: &str) -> Vec<EffectParameter> {
    match name {
        "3-Band EQ" => vec![
            EffectParameter::Mix,
            EffectParameter::LowDb,
            EffectParameter::MidDb,
            EffectParameter::HighDb,
        ],
        "Reverb" | "Plate Reverb" => vec![
            EffectParameter::Mix,
            EffectParameter::Amount,
            EffectParameter::RoomSize,
        ],
        "Delay" | "Tape Echo" => vec![
            EffectParameter::Mix,
            EffectParameter::Amount,
            EffectParameter::TimeSeconds,
            EffectParameter::Feedback,
        ],
        "Juno Chorus" | "Phaser" => vec![
            EffectParameter::Mix,
            EffectParameter::Amount,
            EffectParameter::RateHz,
            EffectParameter::Depth,
        ],
        "Waveshaper" => vec![EffectParameter::Mix, EffectParameter::Drive],
        _ => vec![EffectParameter::Mix],
    }
}

fn parameter_target_label(project: &Project, target: ParameterTarget) -> String {
    match target {
        ParameterTarget::Instrument {
            instrument_id,
            parameter,
        } => format!(
            "{} · {}",
            project
                .instruments
                .get(instrument_id)
                .map_or("Instrument", |instrument| instrument.name.as_str()),
            parameter.label()
        ),
        ParameterTarget::PluginParameter {
            host_id,
            parameter_id,
        } => format!("Plug-in {host_id} \u{00B7} parameter {parameter_id}"),
        ParameterTarget::MixerChannel {
            channel_id,
            parameter,
        } => format!(
            "{} \u{00B7} {}",
            if channel_id == project.lanes.len() {
                "Master".to_owned()
            } else {
                project
                    .lanes
                    .get(channel_id)
                    .map_or_else(|| "Mixer".to_owned(), |lane| lane.name.clone())
            },
            parameter.label()
        ),
        ParameterTarget::Tempo => "Tempo".to_owned(),
        ParameterTarget::InsertEffect {
            channel_id,
            slot_id,
            parameter,
        } => {
            let channel_name = if channel_id == project.lanes.len() {
                "Master"
            } else {
                project
                    .lanes
                    .get(channel_id)
                    .map_or("Mixer channel", |lane| lane.name.as_str())
            };
            let slot_name = if channel_id == project.lanes.len() {
                project.master.inserts.get(slot_id)
            } else {
                project
                    .lanes
                    .get(channel_id)
                    .and_then(|lane| lane.inserts.get(slot_id))
            }
            .map_or("FX", |slot| slot.name.as_str());
            format!("{channel_name} · {slot_name} · {}", parameter.label())
        }
    }
}

/// Semitone offsets of a named scale, relative to its root.
fn scale_steps_for(name: &str) -> &'static [u8] {
    match name {
        "Major" => &[0, 2, 4, 5, 7, 9, 11],
        "Minor" => &[0, 2, 3, 5, 7, 8, 10],
        "Harmonic minor" => &[0, 2, 3, 5, 7, 8, 11],
        "Dorian" => &[0, 2, 3, 5, 7, 9, 10],
        "Phrygian" => &[0, 1, 3, 5, 7, 8, 10],
        "Lydian" => &[0, 2, 4, 6, 7, 9, 11],
        "Mixolydian" => &[0, 2, 4, 5, 7, 9, 10],
        "Pentatonic major" => &[0, 2, 4, 7, 9],
        "Pentatonic minor" => &[0, 3, 5, 7, 10],
        "Blues" => &[0, 3, 5, 6, 7, 10],
        _ => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    }
}

fn scale_contains_pitch(root: u8, name: &str, pitch: u8) -> bool {
    let degree = (pitch + 12 - (root % 12)) % 12;
    scale_steps_for(name).contains(&degree)
}

/// Nearest pitch inside the scale, preferring the lower note on a tie.
fn snap_pitch_into_scale(root: u8, name: &str, pitch: u8) -> u8 {
    if scale_contains_pitch(root, name, pitch) {
        return pitch;
    }
    for distance in 1..=6u8 {
        if pitch >= distance && scale_contains_pitch(root, name, pitch - distance) {
            return pitch - distance;
        }
        if pitch + distance < 128 && scale_contains_pitch(root, name, pitch + distance) {
            return pitch + distance;
        }
    }
    pitch
}

fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(pitch % 12)],
        i16::from(pitch / 12) - 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulseforge_core::project::StepHit;

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
