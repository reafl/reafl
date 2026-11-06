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
        MIDI_CHANNEL_COUNT, ParameterTarget, PianoNote, Project, SignalWaveform, SynthKind,
        TimeSignature, disconnect_channel,
    },
};
use pulseforge_profiling::{
    Channel, PluginDebug, SystemMonitor, profile_scope, set_thread_channel,
    ui::{AudioDebug, DebugPanel, DebugWorld},
};
use pulseforge_settings::{cli, settings};
use pulseforge_tasks as tasks;
use pulseforge_widgets::{
    anim, automation_bridge, context_menu, control, effect_parameters, eq_curve, icons, knob,
    lane_slider, mixer_strip, power, presets, theme, time_edit, tooltip,
};

use anim::Animated;
use cli::Cli;
use context_menu::{ContextAction, ContextMenu, MenuItem};
use settings::{Chord, Settings, ShortcutAction};

mod actions;
mod app_setup;
mod app_state;
mod automation_actions;
mod automation_display;
mod clips;
mod cursor;
mod geometry;
mod hosted_meta;
mod hosting;
mod input;
mod instruments;
mod labels;
mod layout;
mod project_io;
mod samples;
mod selection;
mod snap;
mod state;
mod transport;
mod update;
mod views;

pub(crate) use app_state::*;
pub(crate) use cursor::*;
pub(crate) use hosted_meta::*;
pub(crate) use layout::*;
pub(crate) use state::*;
pub(crate) use views::timeline::TimelineGeometry;

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
        Box::new(move |creation_context| Ok(Box::new(PulseForgeApp::new(creation_context, &cli)))),
    )
}

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

impl PulseForgeApp {}

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

impl PulseForgeApp {}

/// Static data cached per hosted plug-in, so the wrapper does not re-query the
/// bundle every frame.
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
