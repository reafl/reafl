//! Span recording.
//!
//! A [`Profiler`] owns one lock-free [`ArrayQueue`] that every instrumented
//! thread pushes finished spans into, plus a background worker (see
//! [`crate::flame`]) that folds them into a call tree. Recording itself only
//! touches a thread-local stack and one atomic, so it is safe on the audio
//! thread; when profiling is disabled a scope costs a single relaxed load.
//!
//! ```
//! pulseforge_profiling::install(3000);
//! {
//!     pulseforge_profiling::profile_scope!("demo");
//!     // work
//! }
//! ```

use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_queue::ArrayQueue;

/// Which part of the engine recorded a span. Each channel is aggregated and
/// drawn separately, so audio work never hides behind UI frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    Ui,
    Audio,
    Worker,
    Other,
}

impl Channel {
    pub const ALL: [Self; 4] = [Self::Ui, Self::Audio, Self::Worker, Self::Other];

    /// Short name used by the debug panel tabs.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ui => "UI",
            Self::Audio => "Audio",
            Self::Worker => "Worker",
            Self::Other => "Other",
        }
    }
}

impl Default for Channel {
    fn default() -> Self {
        Self::Other
    }
}

/// One finished, instrumented scope.
#[derive(Clone, Copy, Debug)]
pub struct RawSpan {
    /// Static name of the scope, as written in `profile_scope!`.
    pub name: &'static str,
    /// Nanoseconds since the recorder started.
    pub start_ns: u64,
    pub duration_ns: u64,
    /// Nesting level on the recording thread (0 = outermost).
    pub depth: u16,
    pub channel: Channel,
    /// Short per-thread id, so interleaved threads do not glue their trees
    /// together.
    pub thread: u16,
}

/// Capacity of the span queue. Roughly a second of saturated recording; when
/// the queue overflows the newest spans are dropped and counted.
const QUEUE_CAPACITY: usize = 1 << 16;

/// How often the aggregation worker folds the queue, in milliseconds.
const AGGREGATE_INTERVAL_MS: u64 = 80;

/// The process-wide recorder, created by [`install`].
static PROFILER: OnceLock<Arc<Profiler>> = OnceLock::new();

/// Installs (once) and returns the process-wide profiler.
///
/// `window_ms` is how much history the flame graph keeps. Later calls return
/// the existing recorder, so both the UI and the audio thread can call this
/// without coordination.
pub fn install(window_ms: u64) -> Arc<Profiler> {
    PROFILER
        .get_or_init(|| {
            let profiler = Arc::new(Profiler::new(window_ms));
            profiler.start_worker();
            profiler
        })
        .clone()
}

/// The process-wide profiler, when [`install`] has run.
pub fn profiler() -> Option<&'static Arc<Profiler>> {
    PROFILER.get()
}

/// Records finished spans and publishes aggregated snapshots.
pub struct Profiler {
    queue: ArrayQueue<RawSpan>,
    enabled: AtomicBool,
    window_ms: AtomicU64,
    dropped: AtomicU64,
    published: Mutex<Arc<crate::flame::Snapshot>>,
    epoch: Instant,
}

impl Profiler {
    fn new(window_ms: u64) -> Self {
        Self {
            queue: ArrayQueue::new(QUEUE_CAPACITY),
            enabled: AtomicBool::new(true),
            window_ms: AtomicU64::new(window_ms.clamp(250, 60_000)),
            dropped: AtomicU64::new(0),
            published: Mutex::new(Arc::new(crate::flame::Snapshot::default())),
            epoch: Instant::now(),
        }
    }

    /// Turns collection on or off. Disabled recording skips the timestamp and
    /// queue push entirely.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// History length kept by the flame graph, in milliseconds.
    pub fn window_ms(&self) -> u64 {
        self.window_ms.load(Ordering::Relaxed)
    }

    pub fn set_window_ms(&self, window_ms: u64) {
        self.window_ms
            .store(window_ms.clamp(250, 60_000), Ordering::Relaxed);
    }

    /// Spans dropped because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Most recent aggregated call tree. Cheap: the `Arc` is cloned.
    pub fn snapshot(&self) -> Arc<crate::flame::Snapshot> {
        match self.published.lock() {
            Ok(snapshot) => Arc::clone(&snapshot),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    /// Nanoseconds since the recorder started.
    pub fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    /// Pushes one span, counting it as dropped when the queue is full.
    pub fn record(&self, span: RawSpan) {
        if self.queue.push(span).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Drains pending spans; only the aggregation worker calls this.
    fn drain(&self, into: &mut Vec<RawSpan>) {
        while let Some(span) = self.queue.pop() {
            into.push(span);
        }
    }

    fn start_worker(self: &Arc<Self>) {
        let profiler = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("pulseforge-profile".to_owned())
            .spawn(move || {
                let mut drained = Vec::with_capacity(4096);
                let mut window: Vec<RawSpan> = Vec::with_capacity(1 << 14);
                loop {
                    thread::sleep(Duration::from_millis(AGGREGATE_INTERVAL_MS));
                    drained.clear();
                    profiler.drain(&mut drained);
                    if !profiler.enabled() {
                        window.clear();
                        continue;
                    }
                    window.extend_from_slice(&drained);
                    let now = profiler.now_ns();
                    let window_ms = profiler.window_ms();
                    let cutoff = now.saturating_sub(window_ms.saturating_mul(1_000_000));
                    let keep_from = window
                        .iter()
                        .position(|span| span.start_ns + span.duration_ns >= cutoff)
                        .unwrap_or(window.len());
                    window.drain(..keep_from);
                    let snapshot =
                        crate::flame::build(&window, window_ms as f32 / 1000.0, profiler.dropped());
                    if let Ok(mut published) = profiler.published.lock() {
                        *published = Arc::new(snapshot);
                    }
                }
            });
    }
}

thread_local! {
    /// Open scopes on this thread: name and start time.
    static STACK: RefCell<Vec<(&'static str, u64)>> = const { RefCell::new(Vec::new()) };
    /// Channel and short id of this thread.
    static THREAD: Cell<(Channel, u16)> = const { Cell::new((Channel::Other, 0)) };
}

/// Monotonic counter behind the short thread ids.
static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);

/// Tells the recorder which channel this thread belongs to.
///
/// Call once at the start of every instrumented thread; later calls on the same
/// thread are ignored so the id stays stable.
pub fn set_thread_channel(channel: Channel) {
    THREAD.with(|cell| {
        let (_, id) = cell.get();
        if id == 0 {
            let id = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed) as u16;
            cell.set((channel, id));
        }
    });
}

/// Channel and id assigned to the calling thread.
pub fn thread_channel() -> (Channel, u16) {
    THREAD.with(Cell::get)
}

/// Records the end of a scope; created by `profile_scope!`.
#[must_use = "the scope ends when the guard is dropped"]
pub struct Scope {
    name: &'static str,
    start_ns: u64,
    profiling: bool,
}

impl Scope {
    /// Opens a scope. `name` must be a `&'static str` (the macros guarantee it).
    pub fn enter(name: &'static str) -> Self {
        let Some(profiler) = profiler() else {
            return Self {
                name,
                start_ns: 0,
                profiling: false,
            };
        };
        if !profiler.enabled() {
            return Self {
                name,
                start_ns: 0,
                profiling: false,
            };
        }
        let start_ns = profiler.now_ns();
        STACK.with(|stack| stack.borrow_mut().push((name, start_ns)));
        Self {
            name,
            start_ns,
            profiling: true,
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if !self.profiling {
            return;
        }
        let Some(profiler) = profiler() else {
            return;
        };
        let (depth, channel, thread) = STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let depth = stack.len().saturating_sub(1);
            stack.pop();
            let (channel, thread) = thread_channel();
            (depth as u16, channel, thread)
        });
        let duration_ns = profiler.now_ns().saturating_sub(self.start_ns);
        profiler.record(RawSpan {
            name: self.name,
            start_ns: self.start_ns,
            duration_ns,
            depth,
            channel,
            thread,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_profiler_never_pushes() {
        let profiler = Profiler::new(1000);
        profiler.set_enabled(false);
        assert!(!profiler.enabled());
        assert_eq!(profiler.dropped(), 0);
        assert_eq!(profiler.snapshot().channels.len(), 0);
    }

    #[test]
    fn queue_overflow_is_counted() {
        let profiler = Profiler::new(1000);
        for index in 0..QUEUE_CAPACITY + 8 {
            profiler.record(RawSpan {
                name: "overflow",
                start_ns: index as u64,
                duration_ns: 1,
                depth: 0,
                channel: Channel::Other,
                thread: 0,
            });
        }
        assert!(profiler.dropped() >= 8);
    }

    #[test]
    fn thread_channel_is_stable() {
        set_thread_channel(Channel::Audio);
        let (channel, id) = thread_channel();
        assert_eq!(channel, Channel::Audio);
        set_thread_channel(Channel::Ui);
        assert_eq!(thread_channel(), (Channel::Audio, id));
    }
}
