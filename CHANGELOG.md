# Changelog

All notable changes to PulseForge. Commits written by AI are marked `AI:`.

## Unreleased

### Added

- `pulseforge-profiling` crate: an in-editor debugger.
  - Lock-free span recorder (`profile_scope!`, `profile_span!`) that is safe on
    the audio thread and costs one atomic load while disabled.
  - Background aggregator that folds the last few seconds of spans into a
    per-channel flame graph (UI, Audio, Worker, Other).
  - CPU/RAM sampler on its own thread (`sysinfo`), including process RSS.
  - Debug panel window (Ctrl+Shift+D, `--debug`): frame time, FPS, worst frame,
    audio DSP load and peak, minute/live graphs, flame graph with inclusive /
    exclusive times, and a hosted plug-in table.
  - GPU frame-graph reduction: a GL 4.3 compute shader bins the sample history
    into min/max/mean columns, with an identical CPU fallback.
- Audio debugging: the output callback publishes its real-time load (work vs
  buffer period) and block size; hosted VST3/CLAP slots record per-block time,
  peak time and error messages without locking the audio path.
- Plug-in debugging: host load failures are kept per slot and shown in the
  debug panel; `--no-audio` starts the UI without instantiating any plug-in.
- Command-line options (`clap`): `--settings/-s`, `--project/-p`, `--debug/-d`
  and `--no-audio`, with `--help` and `--version`.
- `tasks` module: background threads for plug-in scans, sample-folder scans,
  WAV decoding, and project save/load, drained once per frame.

### Changed

- Plug-in folder scanning, sample listing, WAV decoding for instruments, clips
  and previews, and project read/write no longer block the UI thread. Status
  text and the footer report running background jobs.
- Settings and project paths come from the command line, so a portable install
  can use its own files.
- `Settings` gains the `WindowDebug` shortcut (default `Ctrl+Shift+D`).
- `HostedSlot::new` takes optional debug counters; `AudioCommand::LoadHosted`
  carries them to the audio thread.
