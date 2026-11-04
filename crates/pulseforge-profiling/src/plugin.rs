//! Debug counters for hosted VST3/CLAP slots.
//!
//! Every hosted slot owns an [`Arc<SlotCounters>`]; the audio thread updates
//! plain atomics while rendering, and the debug panel reads them once a frame.
//! Nothing here locks on the audio thread, and a plug-in that reports an error
//! deposits the message in a mutex only on the slow path.
//!
//! The panel derives "CPU per plug-in" from the deltas between two reads, so
//! the counters themselves only ever grow.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

/// What a hosted slot is: a generator or an insert effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PluginKind {
    Instrument,
    Effect,
}

impl PluginKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Instrument => "Instrument",
            Self::Effect => "Effect",
        }
    }
}

/// Live counters written by the audio thread. Plain atomics, no allocation.
#[derive(Debug, Default)]
pub struct SlotCounters {
    blocks: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
    last_ns: AtomicU64,
    errors: AtomicU64,
    last_error: Mutex<Option<String>>,
    /// Frames per block the slot was activated with, for the load percentage.
    block_frames: AtomicU64,
    sample_rate: AtomicU64,
}

impl SlotCounters {
    pub fn new(block_frames: u32, sample_rate: u32) -> Arc<Self> {
        Arc::new(Self {
            block_frames: AtomicU64::new(u64::from(block_frames).max(1)),
            sample_rate: AtomicU64::new(u64::from(sample_rate).max(1)),
            ..Self::default()
        })
    }

    /// Records one processed block. `nanos` is the wall time the plug-in spent
    /// inside `process`.
    pub fn record_block(&self, nanos: u64) {
        self.blocks.fetch_add(1, Ordering::Relaxed);
        self.total_ns.fetch_add(nanos, Ordering::Relaxed);
        self.last_ns.store(nanos, Ordering::Relaxed);
        // Peak is advisory, so a lost update under contention is harmless.
        self.max_ns.fetch_max(nanos, Ordering::Relaxed);
    }

    /// Records a failure reported by the plug-in wrapper.
    pub fn record_error(&self, message: &str) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last) = self.last_error.lock() {
            *last = Some(message.to_owned());
        }
    }

    /// Wall time one block of this slot is expected to take, in nanoseconds.
    pub fn block_budget_ns(&self) -> u64 {
        let frames = self.block_frames.load(Ordering::Relaxed);
        let rate = self.sample_rate.load(Ordering::Relaxed);
        frames * 1_000_000_000 / rate
    }

    pub fn blocks(&self) -> u64 {
        self.blocks.load(Ordering::Relaxed)
    }

    pub fn total_ns(&self) -> u64 {
        self.total_ns.load(Ordering::Relaxed)
    }

    pub fn max_ns(&self) -> u64 {
        self.max_ns.load(Ordering::Relaxed)
    }

    pub fn last_ns(&self) -> u64 {
        self.last_ns.load(Ordering::Relaxed)
    }

    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|error| error.clone())
    }
}

/// One row of the plug-in table read by the debug panel.
#[derive(Clone, Debug)]
pub struct PluginSample {
    pub host_id: u64,
    pub name: String,
    pub path: String,
    pub kind: PluginKind,
    /// Blocks processed per second since the previous sample.
    pub blocks_per_second: f32,
    /// Share of one core the plug-in used since the previous sample, 0..=n.
    pub cpu_percent: f32,
    /// Mean time per block, in microseconds.
    pub mean_micros: f32,
    pub peak_micros: f32,
    /// Load against the real-time budget of one block, 0..=n.
    pub realtime_load: f32,
    pub errors: u64,
    pub last_error: Option<String>,
}

/// UI-side registry of hosted slots, plus the previous read used for deltas.
#[derive(Debug, Default)]
pub struct PluginDebug {
    entries: HashMap<u64, Entry>,
    previous: HashMap<u64, (u64, u64, std::time::Instant)>,
    last_sample: Option<std::time::Instant>,
}

#[derive(Debug)]
struct Entry {
    name: String,
    path: String,
    kind: PluginKind,
    counters: Arc<SlotCounters>,
}

impl PluginDebug {
    /// Registers a slot. Called on the UI thread when a plug-in finishes
    /// loading; the returned counters travel to the audio thread inside the
    /// load command.
    pub fn register(
        &mut self,
        host_id: u64,
        name: impl Into<String>,
        path: impl Into<String>,
        kind: PluginKind,
        block_frames: u32,
        sample_rate: u32,
    ) -> Arc<SlotCounters> {
        let counters = SlotCounters::new(block_frames, sample_rate);
        self.entries.insert(
            host_id,
            Entry {
                name: name.into(),
                path: path.into(),
                kind,
                counters: Arc::clone(&counters),
            },
        );
        self.previous.remove(&host_id);
        counters
    }

    pub fn unregister(&mut self, host_id: u64) {
        self.entries.remove(&host_id);
        self.previous.remove(&host_id);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Snapshot of every hosted slot, with rates derived from the previous
    /// call. Call once per frame.
    pub fn sample(&mut self) -> Vec<PluginSample> {
        let now = std::time::Instant::now();
        self.last_sample = Some(now);
        let mut samples: Vec<PluginSample> = self
            .entries
            .iter()
            .map(|(&host_id, entry)| {
                let counters = &entry.counters;
                let blocks = counters.blocks();
                let total = counters.total_ns();
                let (previous_blocks, previous_ns, previous_at) = self
                    .previous
                    .get(&host_id)
                    .copied()
                    .unwrap_or((blocks, total, now));
                let seconds = (now - previous_at).as_secs_f32().max(f32::EPSILON);
                let block_delta = blocks.saturating_sub(previous_blocks) as f32;
                let ns_delta = total.saturating_sub(previous_ns) as f32;
                self.previous.insert(host_id, (blocks, total, now));
                let mean_ns = if block_delta > 0.0 {
                    ns_delta / block_delta
                } else {
                    0.0
                };
                let budget = counters.block_budget_ns().max(1) as f32;
                PluginSample {
                    host_id,
                    name: entry.name.clone(),
                    path: entry.path.clone(),
                    kind: entry.kind,
                    blocks_per_second: block_delta / seconds,
                    cpu_percent: ns_delta / seconds / 1e7,
                    mean_micros: mean_ns / 1_000.0,
                    peak_micros: counters.max_ns() as f32 / 1_000.0,
                    realtime_load: mean_ns / budget,
                    errors: counters.errors(),
                    last_error: counters.last_error(),
                }
            })
            .collect();
        samples.sort_by(|left, right| {
            right
                .cpu_percent
                .partial_cmp(&left.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_accumulate_and_peak() {
        let counters = SlotCounters::new(64, 48_000);
        counters.record_block(1_000);
        counters.record_block(3_000);
        counters.record_block(2_000);
        assert_eq!(counters.blocks(), 3);
        assert_eq!(counters.total_ns(), 6_000);
        assert_eq!(counters.max_ns(), 3_000);
        assert_eq!(counters.last_ns(), 2_000);
        assert_eq!(counters.block_budget_ns(), 64 * 1_000_000_000 / 48_000);
    }

    #[test]
    fn errors_are_kept() {
        let counters = SlotCounters::new(64, 48_000);
        assert!(counters.last_error().is_none());
        counters.record_error("crashed");
        assert_eq!(counters.errors(), 1);
        assert_eq!(counters.last_error().as_deref(), Some("crashed"));
    }

    #[test]
    fn registry_reports_one_row_per_slot() {
        let mut debug = PluginDebug::default();
        let counters = debug.register(7, "Test", "test.vst3", PluginKind::Effect, 64, 48_000);
        counters.record_block(2_000);
        let rows = debug.sample();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].host_id, 7);
        assert_eq!(rows[0].kind, PluginKind::Effect);
        debug.unregister(7);
        assert!(debug.is_empty());
    }
}
