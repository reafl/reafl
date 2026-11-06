//! The PulseForge editor: application state, windows and panels.
//!
//! This crate owns everything that needs to know about a loaded project: the
//! [`PulseForgeApp`] type, the arrangement, mixer, piano roll, channel rack and
//! automation views, the plug-in windows and the glue back to the audio engine.
//! Reusable drawing code lives in `pulseforge-widgets`, settings and background
//! jobs in their own crates, so this one is only the shell.
//!
//! [`run`] is the entry point used by the `pulseforge` binary.

#![warn(missing_docs)]

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
mod debug_panel;
mod editing;
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

#[cfg(test)]
mod tests;

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
