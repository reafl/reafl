//! Background jobs for reafl's blocking work.
//!
//! Scanning plug-in folders, listing sample libraries, decoding audio files and
//! reading or writing a project all block for an unpredictable time. The runner
//! here moves each of them to a scratch thread and hands the result back as one
//! [`Task`] value that the UI drains once per frame.
//!
//! Every job tags its thread for the profiler, so background stalls show up in
//! the flame graph under the `Worker` channel instead of freezing a frame.

mod tasks;

pub use tasks::{SampleUse, Task, Tasks};
