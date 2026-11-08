//! Background work for blocking operations.
//!
//! Scanning plug-in folders, listing sample libraries, decoding WAV files and
//! reading or writing a project are all blocking: a hundred bundles on a slow
//! disk used to freeze the first frames, and a long sample stalled playback
//! preview. Each of those now runs on a scratch thread and returns one
//! [`Task`] value through a channel that the UI drains once per frame.
//!
//! The helper is deliberately tiny — a `std::thread` per job and one `mpsc`
//! channel — because jobs are user-driven and infrequent. Every job tags its
//! thread for the profiler, so background stalls show up in the flame graph
//! under the `Worker` channel.

use std::{
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
};

use reafl_core::{audio::SampleData, project::Project};
use reafl_plugins::ExternalPlugin;
use reafl_profiling::{Channel, profile_scope, set_thread_channel};

/// What a finished job produced.
pub enum Task {
    PluginsScanned {
        plugins: Vec<ExternalPlugin>,
        error: Option<String>,
    },
    SamplesScanned {
        files: Vec<String>,
    },
    SampleDecoded {
        path: String,
        purpose: SampleUse,
        result: Result<SampleData, String>,
    },
    ProjectLoaded {
        path: String,
        result: Result<Box<Project>, String>,
    },
    ProjectSaved {
        path: String,
        result: Result<(), String>,
    },
}

/// Why a sample was decoded, so the result is applied to the right place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleUse {
    /// Loaded into an instrument's sample slot.
    Instrument(usize),
    /// Cached for an arrangement audio clip.
    Clip,
    /// Auditioned once through the master bus.
    Preview,
}

/// Spawns jobs and hands their results back to the UI thread.
pub struct Tasks {
    sender: Sender<Task>,
    receiver: Receiver<Task>,
    running: usize,
}

impl Default for Tasks {
    fn default() -> Self {
        Self::new()
    }
}

impl Tasks {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            running: 0,
        }
    }

    /// Runs `job` on a new thread. The closure owns its inputs, so callers
    /// clone what they need instead of borrowing `self`.
    pub fn spawn<F>(&mut self, job: F)
    where
        F: FnOnce() -> Task + Send + 'static,
    {
        self.running += 1;
        let sender = self.sender.clone();
        let spawned = thread::Builder::new()
            .name("reafl-job".to_owned())
            .spawn(move || {
                set_thread_channel(Channel::Worker);
                profile_scope!("background job");
                let task = job();
                // A closed channel only means the app is shutting down.
                let _ = sender.send(task);
            });
        if spawned.is_err() {
            // Thread creation can fail under memory pressure; the job is lost
            // rather than blocking the UI.
            self.running -= 1;
        }
    }

    /// Jobs that have not reported yet.
    pub fn running(&self) -> usize {
        self.running
    }

    /// Next finished job, if any. Call in a loop until it returns `None`.
    pub fn try_recv(&mut self) -> Option<Task> {
        match self.receiver.try_recv() {
            Ok(task) => {
                self.running = self.running.saturating_sub(1);
                Some(task)
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }

    /// Decodes a WAV on a worker thread.
    pub fn decode_sample(&mut self, path: String, purpose: SampleUse) {
        self.spawn(move || {
            let result = reafl_core::audio::read_wav(&path);
            Task::SampleDecoded {
                path,
                purpose,
                result,
            }
        });
    }

    /// Scans plug-in folders on a worker thread.
    pub fn scan_plugins(&mut self, roots: Vec<String>, settings_missing: bool) {
        self.spawn(move || {
            let plugins = reafl_plugins::scan_plugins(&roots, 512);
            let error = if settings_missing && plugins.is_empty() {
                Some(
                    "No plug-in folders configured yet. Add VST/VST3/CLAP locations in Settings · Plug-ins."
                        .to_owned(),
                )
            } else {
                None
            };
            Task::PluginsScanned { plugins, error }
        });
    }

    /// Lists `.wav` files under the configured sample folders.
    pub fn scan_samples(&mut self, roots: Vec<String>) {
        self.spawn(move || Task::SamplesScanned {
            files: collect_samples(&roots, 2_000),
        });
    }

    /// Serializes and writes a project snapshot.
    pub fn save_project(&mut self, path: String, project: Project) {
        self.spawn(move || {
            let result = serde_json::to_string_pretty(&project)
                .map_err(|error| format!("Could not serialize project: {error}"))
                .and_then(|json| {
                    std::fs::write(&path, json)
                        .map_err(|error| format!("Could not save project: {error}"))
                });
            Task::ProjectSaved { path, result }
        });
    }

    /// Reads and parses a project file.
    pub fn load_project(&mut self, path: String) {
        self.spawn(move || {
            let result = std::fs::read_to_string(&path)
                .map_err(|error| format!("Could not read project: {error}"))
                .and_then(|contents| {
                    serde_json::from_str::<Project>(&contents)
                        .map_err(|error| format!("Invalid project JSON: {error}"))
                })
                .map(Box::new);
            Task::ProjectLoaded { path, result }
        });
    }
}

/// Depth-first `.wav` collection, capped so a huge library cannot stall a job.
fn collect_samples(roots: &[String], limit: usize) -> Vec<String> {
    let mut files = Vec::new();
    for root in roots {
        if files.len() >= limit {
            break;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_wav = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"));
            if is_wav && path.is_file() {
                files.push(path.to_string_lossy().into_owned());
                if files.len() >= limit {
                    break;
                }
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawned_jobs_report_exactly_once() {
        let mut tasks = Tasks::new();
        tasks.spawn(|| Task::SamplesScanned {
            files: vec!["a.wav".to_owned()],
        });
        assert_eq!(tasks.running(), 1);
        let mut received = 0;
        // The worker needs a moment; poll briefly like the UI does.
        for _ in 0..200 {
            while let Some(task) = tasks.try_recv() {
                matches!(task, Task::SamplesScanned { .. });
                received += 1;
            }
            if received == 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(received, 1);
        assert_eq!(tasks.running(), 0);
    }

    #[test]
    fn missing_folders_produce_no_samples() {
        let files = collect_samples(&["definitely/not/a/folder".to_owned()], 10);
        assert!(files.is_empty());
    }
}
