# Changelog

All notable changes to reafl. Commits written by AI are marked `AI:`.

Every batch of changes is committed to git as its own commit whose message
starts with `AI: `, so history maps one commit to one reviewed batch.

## Unreleased

### Added

- **Track unification (breaking).** Every row of the arrangement is now one
  `Track` that is simultaneously an arrangement row, a mixer strip and a rack
  slot. `Project::instruments`, `Project::lanes` and `Project::patterns` are
  replaced by `Project::tracks`; a clip owns its notes, so `ClipSource`,
  `Pattern`, `StepHit` and `ClipNote::instrument_id` are gone. Project files
  from the previous shape are intentionally not migrated.
- **Folder tracks.** A track with children has no generator: it applies its own
  `midi_fx` to a clip and plays it through every descendant, and its
  gain/pan/width/EQ/inserts process the summed children. Nesting is arbitrary;
  `reparent_track` refuses cycles. `processing_order` renders children before
  parents and zeroes each child buffer once it has been summed, so the tree
  ordering is also the mixer routing.
- **Group automation.** A folder offers its descendants' common generator
  parameters in `group_targets` (`available_group_targets` filters to the
  intersection over every descendant, skipping nested folders), and all mixer
  and MIDI FX parameters are group-wide without configuration
  (`TrackParameter::is_group_wide`). Values resolve own lane → nearest ancestor
  group lane → base, in both `Project::effective_track_value` and the engine's
  `AudioEngine::resolve_track_value`.

### Changed

- Automation targets collapsed into `ParameterTarget::Track { track_id,
  parameter }` with `TrackParameter::{Generator, MidiFx, Mixer}`, alongside the
  existing clip and sample-default targets; the instrument/mixer-channel
  families are gone. `normalize` prunes lanes whose track or parameter
  disappeared, and `add_track`/`add_group` shift master-insert lanes so they
  keep addressing the master.
- The editor's rack, arrangement, mixer, piano roll and automation picker are
  per-track views over the same index space, so a track id means the same thing
  in every window. The rack paints steps onto the selected clip and the piano
  roll edits the clip's own notes.
- The default project is a `Drums` folder holding Kick, Snare and Hat plus a
  standalone `Synth`, each with a clip.

- Track model groundwork for the unified workflow: `Track` (generator or group,
  MIDI FX, mixer, clips), `Generator`, `GeneratorKind`, `MidiFx`, `TrackMixer`,
  `Clip` and `ClipNote`, plus tree helpers (`children`, `descendants`,
  `ancestors`, depth, cycle-guarded reparenting).
- `TrackParameter` / `MidiFxParameter` and `Track::automation_parameters`: a
  track now lists its own automatable controls, and a group also offers the
  parameters its children have in common (`Track::available_group_targets`,
  `Track::group_targets`). Mixer and MIDI FX controls are group-wide; a
  descendant's own lane still wins. See `docs/track-unification.md`.
- Parameter ranges moved onto the parameter enums (`InstrumentParameter::range`,
  `MixerParameter::range`, `MidiFxParameter::range`) so `ParameterTarget` and the
  new track targets share one definition.

### Changed

- The workspace is split into one crate per concern: `reafl-core` (model and
  engine), `reafl-plugins`, `reafl-hosting`, `reafl-profiling`,
  `reafl-widgets` (all shared egui drawing), `reafl-settings`
  (settings + CLI), `reafl-tasks` (background jobs) and `reafl-app`
  (the editor). The `reafl` binary is now a thin `run()` call.
- `reafl-core` is modularised: `project/` holds `model` (data), `behaviour`
  (normalising, automation lookup, lane edits) and tests, while `audio/` holds
  `model` (samples, commands, voices), `engine` (the renderer), `dsp` (free
  helpers and WAV I/O) and tests.
- `reafl-plugins` built-ins are now one module per processor behind a
  `BuiltinProcessor` trait: `builtin/{eq,reverb,delay,modulation,waveshaper,params}.rs`,
  with `BuiltinEffect` only dispatching, so a new effect is one file plus one
  line in the constructor.
- `reafl-app` no longer holds one giant file: the `ReaflApp` methods
  live in `views/{arrangement,automation,mixer,panels,piano,plugins,rack,settings,toolbar}.rs`
  plus `actions`, `app_setup`, `automation_actions`, `automation_display`, `clips`,
  `hosting`, `input`, `instruments`, `project_io`, `samples`, `selection`,
  `transport`, and the shared helpers in `geometry`, `labels`, `snap`.
- New `reafl_widgets::control::ValueControl`: one control type for sliders,
  knobs, number boxes, toggles and enums, each bound to a `&mut` value with an
  optional `ParameterTarget` (automation ring, live value, lane menu) and an
  optional `on_change` callback. The arpeggiator gate is the first user.
- New `reafl_widgets::shape`: `DrawPoint`/`PointStyle` for lane points,
  `ScreenCurve` (line, fill and cable) and the samplers `sample_curve`,
  `value_at` and `eq_response_curve` shared by the EQ display and the lanes.
- Every crate is now organised as one module per concern, so no file carries a
  whole subsystem: the project model lives in
  `project/model/{params,piano,device,targets,automation,arrangement}.rs`, its
  behaviour in `project/behaviour/{normalize,automation,lanes,steps}.rs`, the
  audio side in `audio/{model,sample,output,dsp}` plus
  `audio/engine/{mod,render,voices,commands,automation,effects,setup}.rs`, the
  settings crate in `settings/{mod,shortcuts,appearance,tests}.rs`, and the app
  views in `views/{timeline,timeline_paint,timeline_rows,timeline_input}`,
  `views/{automation,automation_target,automation_formula,automation_tracks}`,
  `views/{piano,piano_grid,piano_input,piano_gestures,piano_velocity}`,
  `views/{settings,sample_browser,clip_windows}` and `editing`, `debug_panel`,
  `tests`. `views/timeline.rs` hands the frame geometry to the painting and
  input passes as an immutable `TimelineGeometry`.
- `reafl-core`, `reafl-plugins`, `reafl-app` and
  `reafl-widgets` carry `#![warn(missing_docs)]` and are fully documented:
  every public type, field, variant and method has rustdoc, with `# Parameters`,
  `# Returns` and `# Errors` sections where they apply, so `cargo doc` produces
  the reference without gaps.

### Added

- The automation window's target picker now has four modes instead of two:
  **Instrument**, **Insert FX**, **Mixer** (gain, pan, width, EQ low/mid/high
  and a send per destination) and **Clip** (a clip's volume/pan/pitch or the
  project-wide sample defaults). `AutomationTargetKind` replaces the old
  `automation_effect_mode` flag, `focus_automation_target` points the picker at
  whatever a control's context menu asked for, and the formula tool uses the
  same selection instead of assuming an instrument target. A control's "Add
  automation lane" now also stops creating two lanes for global targets.
- Automation coverage for the controls that had none left: the **channel EQ**
  (`MixerParameter::LowDb/MidDb/HighDb`, folded into the engine's cached filter
  coefficients), **sends** (`MixerParameter::Send { destination }`, which can
  even introduce a send the project does not store), and the **audio clip**
  controls plus their project-wide defaults (`ParameterTarget::Clip { lane_id,
  clip_id, parameter }` and `::SampleDefaults`, with `ClipParameter::{Gain, Pan,
  Pitch}`). Lanes for removed clips are pruned by `Project::normalize`, and the
  automation window groups clip lanes under their lane and sample defaults under
  the master.
- Envelope times of an automation lane are automatable too:
  `AutomationLaneParameter::{Attack, Decay, Release}` cover the MIDI envelope and
  the track-peak follower, and the rate clamp now follows the rate's own unit, so
  automating a 500 ms LFO rate no longer squeezes it into 64 ms.
- `AutomationSource::Peak { channel_id, post_fx, attack, release, base, depth }`:
  a lane can follow the peak level of any mixer track, read either before its
  insert chain (pre FX) or after it (post FX), with its own follower attack and
  release. The engine keeps a per-block maximum per channel plus a per-lane
  follower, so two lanes can read the same track at different speeds, and
  `base`/`depth` stay automatable like every other source. Because the value
  only exists during playback, the lane draws a blank **placeholder clip** whose
  caption names the track and the tap point ("Peak of Track 02 (post FX)").
- Automation of an automation lane's own parameters is now visible where the
  lane lives: the arrangement clip carries a `↺ base` / `↺ depth` badge per
  parameter that another lane drives, the automation window repeats the list
  next to the lane's target, and the drawn envelope samples the *driven* base
  and depth instead of the stored ones, so a modulated lane shows what it
  actually does. `AutomationSource` gained `label`, `has_parameters`, `is_live`,
  `base`, `depth` and `with_base_depth` so the views do not duplicate that logic.
- Sample and hold is its own automation source
  (`AutomationSource::SampleAndHold { seed, rate, base, depth }`) with its own
  picker entry, lane preview and parameter row (rate, base, depth, seed).
  Existing projects that used `LfoShape::SampleAndHold` still load the same
  values, but the LFO shape list no longer offers it.
- The mixer fader, the mixer pan knob and the toolbar BPM box are automatable:
  they create `MixerChannel { Gain | Pan }` and `Tempo` lanes through a shared
  `automation_context_menu` helper, and an automated fader draws the knob's
  purple ring plus a dot at the engine's live value.
- Relative automation: the new `AutomationSource::RelativeCurve` draws offsets
  from -1 to 1 (around a zero line) that are added to the parameter's own value
  instead of replacing it, so several relative lanes can stack over an absolute
  one. Negative point values now survive `Project::normalize` and the
  `automated_value` paths handle the offset both in the engine and in the UI.
- Automation coverage for the remaining values:
  `MixerParameter::Width` gives the mixer stereo width knob a lane (the engine
  gained a smoothed `lane_width`), `InstrumentParameter::Waveform` and
  `::ArpMode` automate enums as plain integer indices, and
  `ParameterTarget::AutomationLane` automates a lane's own base, depth, rate
  amount and LFO shape. Shared `automation_context_menu` / and a
  `lane_parameter_slider` helper put the "Add automation lane..." entry on the
  width, waveform, arp-mode and lane-source controls.
- The arpeggiator's own values are automatable too: `InstrumentParameter::ArpGate`,
  `::ArpOctaves` (rounded) and `::ArpRate` join `::ArpMode` and `::Waveform`, so
  the whole per-instrument value set now has targets.

### Fixed

- **The workspace builds on Linux again.** `Vst3Ui::open_editor` called
  `WindowHandle::from_hwnd`, which `vst3-host` only defines on Windows, so the
  Linux job failed to compile `reafl-hosting`. A new `window_handle` helper
  picks the constructor per platform — `from_hwnd` on Windows, `from_nsview` on
  macOS, `from_x11` on Linux — and returns [`HostError::NoEditor`] where no
  embedding backend exists. Native editor windows still only exist on Windows;
  elsewhere the wrapper reports it instead of failing to build.
- Linux plug-in discovery now also scans the per-user and `/usr/local` locations
  installers actually use (`~/.vst`, `~/.vst3`, `~/.clap`,
  `/usr/local/lib/{vst,vst3,clap}`) instead of only `/usr/lib`.
- **Solo and mute now follow the track tree.** Solo used to compare the solo
  flags of two flat lists, so soloing a child inside a folder silenced it (its
  folder was not soloed and dropped the submix) and soloing a folder silenced
  its children. `track_audible` resolves solo against the whole subtree — a
  folder stays in the signal path while any descendant is soloed, a soloed
  folder lets every descendant through, and disabling a folder silences its
  children (and their sends).
- **Live MIDI on a folder now plays.** `handle_midi_note` fanned out only for
  clips, so routing a MIDI channel to a folder produced silence. A note on a
  folder now fans out to every source track below it through the folder's MIDI
  FX, and the note-off releases the same transposed pitch.
- `group_track` inserted the new folder at the end of the track list instead of
  directly above the track it wraps, so the arrangement drew the folder after
  its child. The folder is now placed above it (and the returned index is the
  folder's real index).
- `remove_track` only removed the *first* automation lane that targeted the
  deleted track, leaving the rest pointing at whatever track shifted into that
  index. Every lane on the removed track (and on its descendants) is dropped
  now.
- A lane whose own parameters are driven by a *generated* source now updates in
  the UI. The preview resolved lane automation through
  `Project::automated_value`, which only understands drawn curves, so a track
  peak (or LFO, Perlin, envelope, sample & hold) driving another lane's rate,
  shape or envelope times was silently ignored by the drawing. Evaluation now
  goes through one evaluator, `automated_value_at`, that handles every source
  and recurses through lane-on-lane automation with a depth guard — the drawing,
  the live values and the engine all use the same rules.
- Live automation values now match the engine for *every* source, not just
  drawn curves. The UI reproduces the engine's arithmetic (drawn curve replaces
  the value, relative curves offset it, generated sources add a delta from their
  own centre), so LFOs, Perlin noise, MIDI envelopes, sample & hold, peaks and
  curves all move the controls the way they move the sound — driving a
  lane-parameter lane included.
- Track peaks are published from the audio thread (`AudioOutput::peaks`) and
  read back by the UI, because a peak only exists during playback. Peak lanes
  animate while everything else stays prerendered: the arrangement clip fills
  with the follower level ("Peak of Track 02 (post FX) · 62%") and any control a
  peak drives follows it live.
- Automating an automation lane's own parameters now shows up where you look:
  the arrangement preview and the knobs resolve the driven source first, so a
  lane whose **rate**, **shape**, **attack/decay/release**, **sustain** or
  **octaves** is automated draws — and reports — what will be heard instead of
  the stored settings. Previously only `base` and `depth` were folded in, so an
  LFO kept its old frequency and an envelope its old attack in the drawing even
  though the engine already applied them. `lane_driven_parameters` lists what is
  driven and `resolved_lane_source` applies it at a position.
- Enum controls no longer double as an automation menu: a `ComboBox` owns its own
  popup, so the arpeggiator mode, waveform and LFO shape combos keep their
  dropdown and get a compact "A" button beside them that creates the lane
  (click or right-click). Sliders and drag values keep the context-menu entry.
- Automated controls now follow the engine value: live values are published while
  the transport is stopped as well, and the knob dial, fader handle and the
  automatable sliders sit at the automated value instead of the stored one
  (dragging still writes through to the base).
- Automation lanes can be created from more places: the arpeggiator's mode combo,
  gate, octaves and rate, the signal generator's waveform and MIDI note, the
  insert editor's Mix slider, and a lane's own base/depth/rate/shape/sustain/
  octaves all offer "Add automation lane...", and creating one opens the
  automation window.
- Moving an automation clip and drawing its points no longer conflict: the clip
  is grabbed only from its title band (the top 8 px), and a press that lands on a
  point never starts a clip move.
- Automation lanes can select a curve transition again: right-clicking the lane
  offers the curve for the segment under the pointer (the row response used to
  swallow the clip and point menus, so nothing could be picked), and the
  per-point menu names the incoming transition and explains that the first point
  has none.
- Mixer chain presets now include external plug-in state: the inspector captures
  plug-in state before writing the preset, and loading one re-instantiates the
  hosted plug-ins so their parameters are restored (also from the "+ Track"
  preset menu).
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

- Mixer chain presets: the inspector's insert list can be saved to
  `presets/mixer/<name>.json` and loaded onto any channel (or the master). The
  mixer's "+ Track" button opens the preset list on right click and adds a
  channel pre-loaded with that chain.

- `reafl_core::expr`: a dependency-free formula evaluator (`+ - * / % ^`,
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
- Mixer routing send levels use the shared `Knob` widget instead of a hand-drawn
  circle, so they drag, reset, open the same context menu and offer automation
  like every other parameter.
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

- `reafl-profiling` crate: an in-editor debugger.
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
