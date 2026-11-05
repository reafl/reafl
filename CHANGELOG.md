# Changelog

All notable changes to PulseForge. Commits written by AI are marked `AI:`.

## Unreleased

### Fixed

- Step sequencer columns use a fixed width again (`STEP_COLUMN_WIDTH = 24`) so
  every channel row aligns; the rack scrolls instead of squeezing the steps.
- The arrangement wheel scrolls the track rows; Ctrl+wheel zooms. Right-dragging
  deletes clips, and shift+clicking a clip duplicates it.
- Step sequencer buttons update: steps are written as real C5 notes (the rack drew
  note state while the buttons wrote `step_hits`, so toggles were invisible).
- Step columns scale with the rack window instead of a fixed 24 px width.
- Removing an instrument no longer leaves dangling indices: MIDI routes, mixer
  automation targets and `MidiEnvelope` sources shift with the removal (tracks
  that pointed at the removed instrument are dropped).
- Step sequencer: painting a step beyond the pattern length extends the pattern,
  the wheel scrolls across steps, and shortening a pattern no longer wraps or
  hides notes that sit beyond the new length.
- Mixer strip actions (insert enable, ctrl+solo, lock, FX bypass) are applied
  before the project sync, so the engine actually hears them; previously the
  model changed but the audio chain kept the old state.
- Sliced clips draw their contents from the cut point instead of restarting
  the pattern at step 0 (`clip.offset_steps` is honoured).
- Selecting an arrangement clip also selects its pattern, names it in the status
  line, and multi-selected clips use the same outline/lift.
- Right-click (delete mode) clears the selection in the piano roll and the
  arrangement.
- Repeats lane bars snap to whole repeats (0–32) instead of a velocity scale.
- The mixer strip uses a custom dB fader drawn to exactly the meter's height,
  with a filled dB column instead of a slider rail.
- Mixer strips keep a constant height: the insert list has a fixed height.
- Zoom-in is capped at 3×.
- Piano-roll hit-testing now uses the row under the pointer instead of the
  scale-snapped pitch, so notes can be selected, moved and deleted with scale
  snap enabled; the selection is also reported in the status line.
- The Ctrl+drag selection band starts on any Ctrl+drag and selects every note
  whose body it overlaps.
- Clip previews no longer stack every off-screen note at the left edge while
  panning, and only draw notes that fall inside the clip.
- The piano keys mark the scale root with an accent stripe.
- The time-value editor keeps its rate and unit selector on one row.
- Selection with Ctrl: ranges now resolve to entries of the same kind (clips and
  channels used to be looked up in the note list), a first Ctrl+click selects
  the item, and Ctrl+click no longer starts a clip drag.
- Ctrl+drag in the piano roll rubber-band selects notes.
- The tool hotkeys (P/S/B/J/C) set the arrangement tool too, so `S` switches to
  Select outside the piano roll (Join stays piano-roll only).

- Locked power switches (tracks, instruments, insert slots) can be turned off
  and on directly again; the lock now only shields them from solo and other
  global operations.
- The loop, metronome and pattern-playback buttons show whether they are active.
- `Ctrl+A` selects every note in the piano roll (bindable as "Select all notes").
- The mixer strip's pan knob no longer draws a label.
- Ctrl+clicking an insert's power switch solos that insert, like the channel
  strip's own switch.
- Repeated notes draw a divider at every retrigger point in the piano roll.
- The piano roll's cut tool previews the exact split point as a dashed line.
- The clip picker highlights the pattern the arrangement pen will place.

### Added

- `pulseforge_core::expr`: a dependency-free formula evaluator (`+ - * / % ^`,
  `exp ln sqrt abs floor round sin cos tan`, `pi`, `e`, variable `x`) with
  tests, plus a formula field in the automation window that samples an
  expression into a real curve lane.
- The piano-roll control lane is pinned under the grid and can be collapsed
  with the "Lane" toggle.
- The cut tool previews its split point as a dashed line in the arrangement and
  the piano roll.

- Interface scale setting (Settings → Graphics), applied with `Context::set_zoom_factor`
  and stored as `ui_scale`, so the UI can be sized like a DPI setting.
- Ten insert slots per mixer channel and on the master (`INSERT_SLOTS`); the
  strip's insert list scrolls so every slot stays reachable.
- "FX bypass" on each mixer strip: the channel renders dry while every insert
  keeps its settings and enable state.

### Changed

- The mixer inspector is one page (EQ, stereo width, inserts); level and pan
  stay on the strips.
- Each mixer strip has a stereo-width knob backed by a mid/side scale in the
  engine (`ArrangementLane::width`, 0 = mono, 2 = double).
- A "+ Track" button closes the mixer strip row.
### Changed

- One `PowerSwitch` widget (`power_switch` / `power_switch_at`) draws enable
  buttons: plain click powers, shift locks, ctrl solos. The arrangement track
  header and the mixer strip insert rows use it, so they cannot drift in colour
  or behaviour again.
- `ContextAction::ToggleInsertEnabled` carries a `PowerIntent`, so the decision
  is made at click time instead of re-reading the keyboard when the action runs.
- Automation envelopes are sampled on a fixed quarter-step grid, so zooming no
  longer makes MIDI envelopes jitter.
- Automation clips, curves and points are clipped to the timeline grid instead
  of painting over the track header.
- Plug-in settings use two columns (routing next to the arpeggiator).
- The mixer fader is a dB scale down to −100 dB; the bottom of the travel is
  exactly silent.
- The toolbar's zoom slider and Help menu are gone (zoom stays on Ctrl+wheel and
  in Settings → Graphics).
- Horizontal zoom range widened from 0.35–4× to 0.2–8×; Ctrl+click on the
  arrangement ruler resets it to 100 %.

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
