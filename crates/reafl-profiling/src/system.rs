//! CPU and RAM sampling.
//!
//! One detached thread refreshes a [`sysinfo::System`] every 500 ms and stores
//! the result behind a mutex. The UI thread only clones the small
//! [`SystemStats`] struct, so a slow or contended sampler never costs a frame.

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use sysinfo::{Pid, ProcessesToUpdate, System};

/// Sampling period of the background thread.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

/// One reading of machine and process load.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemStats {
    /// Whole-machine CPU usage, 0..=100.
    pub cpu_percent: f32,
    /// Resident memory of reafl, in bytes.
    pub process_ram: u64,
    /// Machine-wide resident memory in bytes.
    pub used_ram: u64,
    pub total_ram: u64,
    /// Seconds since the machine booted.
    pub uptime_seconds: u64,
    /// Threads the process is currently running.
    pub threads: usize,
    /// Number of successful samples, so the UI can tell a stalled sampler.
    pub samples: u64,
}

impl SystemStats {
    pub fn used_ram_percent(&self) -> f32 {
        if self.total_ram == 0 {
            0.0
        } else {
            (self.used_ram as f32 / self.total_ram as f32) * 100.0
        }
    }
}

/// Background sampler; drop it to stop (the thread ends on its next tick).
pub struct SystemMonitor {
    latest: Arc<Mutex<SystemStats>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for SystemMonitor {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl SystemMonitor {
    /// Starts sampling immediately; the first reading is available after a few
    /// CPU refresh intervals, which is why `System::new_all` is not used here.
    pub fn start() -> Self {
        let latest = Arc::new(Mutex::new(SystemStats::default()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let published = Arc::clone(&latest);
        let stop_flag = Arc::clone(&stop);
        let _ = thread::Builder::new()
            .name("reafl-sysmon".to_owned())
            .spawn(move || {
                let Ok(mut system) = System::new() else {
                    return;
                };
                let pid = sysinfo::get_current_pid().ok();
                while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    system.refresh_cpu_usage();
                    system.refresh_memory();
                    if let Some(pid) = pid {
                        let pids = [pid];
                        system.refresh_processes(ProcessesToUpdate::Some(&pids), true);
                    }
                    let stats = SystemStats {
                        cpu_percent: system.global_cpu_usage(),
                        process_ram: pid
                            .and_then(|pid: Pid| system.process(pid))
                            .map_or(0, |process| process.memory()),
                        used_ram: system.used_memory(),
                        total_ram: system.total_memory(),
                        uptime_seconds: System::uptime().unwrap_or(0),
                        threads: thread_count(),
                        samples: 0,
                    };
                    if let Ok(mut latest) = published.lock() {
                        let samples = latest.samples + 1;
                        *latest = SystemStats { samples, ..stats };
                    }
                    thread::sleep(SAMPLE_INTERVAL);
                }
            });
        Self { latest, stop }
    }

    /// Most recent reading.
    pub fn stats(&self) -> SystemStats {
        match self.latest.lock() {
            Ok(stats) => *stats,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }
}

/// Thread count of this process, without pulling in a platform crate.
fn thread_count() -> usize {
    #[cfg(target_os = "linux")]
    {
        return std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find(|line| line.starts_with("Threads:"))
                    .and_then(|line| line.split_whitespace().nth(1))
                    .and_then(|count| count.parse().ok())
            })
            .unwrap_or(0);
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Formats a byte count for the debug panel.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_percentage_is_guarded_against_zero() {
        let stats = SystemStats::default();
        assert_eq!(stats.used_ram_percent(), 0.0);
    }

    #[test]
    fn bytes_are_formatted_with_units() {
        assert_eq!(format_bytes(512), "512.0 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MiB");
    }
}
