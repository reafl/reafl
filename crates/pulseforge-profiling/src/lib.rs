//! In-editor profiling and debugging for PulseForge.
//!
//! The crate is split into four independent layers so the DAW can take only
//! what it needs:
//!
//! * [`span`] — the recorder: scoped spans pushed into a lock-free queue so the
//!   real-time audio thread never blocks. Use [`profile_scope!`] to instrument
//!   a block of code; the macros cost one atomic load when profiling is off.
//! * [`flame`] — the aggregator: a background thread drains the queue and folds
//!   the last few seconds of spans into a per-thread call tree that the UI can
//!   draw as a flame graph.
//! * [`plugin`] — lock-free counters for hosted VST3/CLAP slots (time per
//!   block, peak, errors), so a misbehaving plug-in is visible instead of
//!   mysterious.
//! * [`system`] — CPU and RAM sampling on a dedicated thread (feature
//!   `system`), and [`ui`] — the egui debug panel (feature `ui`).
//!
//! Design notes:
//!
//! * Spans carry `&'static str` names, so recording never allocates and the
//!   flame graph can show real symbols instead of hashes.
//! * Everything shared with the audio thread is lock-free: an `ArrayQueue` for
//!   spans and atomics for counters. No allocation, no mutex, no syscall.
//! * Aggregation is O(spans in window) every ~80 ms on a worker thread, which
//!   keeps the UI frame budget untouched.

pub mod flame;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod plugin;
pub mod span;

#[cfg(feature = "system")]
pub mod system;

#[cfg(feature = "ui")]
pub mod ui;

mod macros;

pub use flame::{ChannelFlame, FlameNode, Snapshot};
pub use plugin::{PluginDebug, PluginKind, PluginSample, SlotCounters};
pub use span::{Channel, Profiler, RawSpan, Scope, install, profiler, set_thread_channel, thread_channel};

#[cfg(feature = "system")]
pub use system::{SystemMonitor, SystemStats};
