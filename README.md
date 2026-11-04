# PulseForge

NOTE: This project uses generative AI as its main development platform (for now). Any commit marked with AI: will be fully written by AI, any commit marked with pAI was partially written.

PulseForge is an original pattern-based DAW prototype written in Rust for
Windows and Linux. It uses a persistent arrangement workspace with separate,
movable channel-rack, piano-roll, mixer, mixer-routing, and MIDI-routing windows.

## Current prototype

- A horizontally pannable arrangement with an open-ended step timeline,
  addable lanes, a clip picker, pattern/audio clips, and drag-to-place routing.
  The playback position is shown as a smoothly animated vertical line, and the
  global snap setting applies to arrangement edits, loops, notes, and automation
  points. Arrangement clips, loops, and automation points use whole-step
  positions; piano notes retain sub-step snap.
- Shared Pen/Brush/Cut tools across the piano roll and the arrangement (Join is
  note-only), each with its own SVG cursor. The arrangement pen places the last
  clicked pattern or audio source, the brush paints repeats, and Cut slices
  clips at the snapped pointer position.
- Project-wide undo/redo (Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z) covering notes,
  patterns, clips, automation lanes, and mixer routing changes.
- A metronome click on the beat and bar, toggled from the toolbar.
- A velocity editor strip below the piano roll grid for per-note dynamics.
- Ctrl+L routes the selected instrument to the next free mixer insert and
  renames that insert after the instrument.
- A settings window (Ctrl+Comma) with Shortcuts, Plug-ins, Samples and
  Interface pages. Settings live in `settings.json`; key bindings are data, so
  `Settings::bind(action, "Ctrl+K")` creates one programmatically and the
  window's capture UI uses the same call.
- External plug-in discovery for VST 2, VST3 and CLAP bundles. Configured
  folders are scanned at startup and on demand; discovered bundles are listed in
  the instrument and effect pickers alongside the built-ins.
- Note repeats: a note can re-trigger up to 32 times, spaced by ticks and losing
  velocity per repeat. Repeats are drawn inside clip previews.
- Arrangement clips draw their contents: note blocks for pattern clips and a
  waveform motif for audio clips, so an empty clip is obvious at a glance.
- Automation clips behave like arrangement clips: drag them horizontally,
  duplicate them, and place several on one parameter in the same row.
- Track rows are resizable by dragging their bottom edge, can be reordered or
  collapsed (hiding their automation rows) from the track header menu, and are
  renamed with F2 or the same menu, which renames the mixer channel too.
- A sample browser (F11) that lists the configured sample folders and auditions
  samples through the master bus without a step sequencer.
- Channel-rack volume and pan knobs per instrument, plus linear smoothing of
  parameter changes (toggleable in Settings ? Interface).
- Interface transitions for clip selection and drag previews, which can be
  disabled or sped up from the settings window. A Graphics page adds "fancy"
  (bouncy) animations, zoom and Alt+wheel step size; Theming and General are
  placeholder pages.
- Power, lock and solo switches on tracks, instruments and insert effects.
  Ctrl+click solos, shift+click unlocks a locked control.
- A shared Select tool: click selects, click on empty space clears, shift
  toggles, ctrl+click extends to a range, and dragging on empty arrangement
  space rubber-band selects clips.
- The step sequencer writes real C5 piano-roll notes, so both editors show the
  same pattern data.
- Audio clips take their length from the loaded sample, and the plugin windows
  gained a shared header that switches between the plug-in and its settings page
  (MIDI in/out, audio in/out, voice count, and an arpeggiator with timesynced or
  free rate, six note orders, gate and octaves).
- The sample player window shows the loaded sample's waveform.
- The piano-roll step ruler is pinned to the top and extends with the pattern
  length, and the control lane switches between velocity and note repeats.
- Tooltips are suppressed by default; pointer details appear in the status bar.
  Settings ? Graphics can instead show hover tooltips after a delay.
- Toolbar icons are painted vectors rather than font glyphs, because the bundled
  fonts do not cover symbols such as U+23FB or U+1F512 (they rendered as boxes).
  The bar uses two fixed-height rows so nothing is clipped, and the snap combo
  participates in the snap shortcuts (Shift+Period / Shift+Comma).
- Arrangement clips can be repeated after themselves with Ctrl+B; pattern
  copies stay linked until a clip is made unique from its context menu.
- Separate instruments and variable-length patterns, stored sparsely so long
  timelines do not require dense step matrices.
- A channel rack with paged steps, left-drag-to-enable/right-drag-to-disable
  painting, and dedicated, resizable plug-in editor windows.
- A paged piano roll with configurable snap, off-grid Alt placement, note
  drawing, moving, resizing, step brush, join, delete, cut, and pan gestures,
  plus a pinned toolbar, step ruler, velocity strip, and its own playhead.
- Numeric time-signature entry, live MIDI input, a sine synth, a waveform
  signal generator, a monophonic dual-oscillator MS-20-inspired synth, a
  Juno-style poly synth, an additive workstation-style voice, and a two-operator
  FM synth.
- A built-in Sample Player with procedural kick, snare, and hi-hat defaults,
  user-loaded WAVs, and arrangement audio clips with sample-rate conversion.
- Offline stereo 16-bit WAV rendering with instruments, samples, automation,
  mixer routing, and effects.
- One mixer channel per arrangement lane. Pattern clips route their instruments
  to the lane where the clip is placed; live MIDI uses each instrument's default
  mixer lane.
- A master mixer channel, per-channel meters, reusable rotary controls, a
  compact-strip mode with a selected-channel inspector, and pin-able channels.
- A separate cable-style routing window with a white, topmost master channel,
  channel-color gradients, hover-adjustable send levels, and right-click
  disconnection for a cable or all connections on a node. Feedback sends are
  delayed by one sample, total absolute incoming send gain is limited to 0.95
  per destination, and master output is peak-limited.
- Built-in insert effects: three-band EQ, room and plate reverb, delay, tape
  echo, Juno-style chorus, phaser, and waveshaper. These are intentionally
  simplified original DSP implementations, not circuit-accurate recreations or
  VST plug-ins.
- A dedicated automation editor (F10) for every continuous built-in instrument
  and insert-effect parameter. Curves support linear, smooth, Bezier, curved,
  pulse, constant, and stepped segments. Lanes can also use clip-bounded
  MIDI-triggered envelopes (retriggered by piano-roll notes and step-sequencer
  hits, including sample instruments), retriggerable or free/synced LFOs, and
  seeded Perlin-noise modulation with 1-16 octaves. Each non-curve source
  supports a drawable 0-1 multiplier curve, drawn in yellow above the source
  curve. Points can be created, moved, shaped, or deleted directly in the
  arrangement; lanes targeting the same parameter are summed. Knobs whose
  parameter has an active automation lane show a violet ring, and during
  playback they display the automated value with a gold marker on the dial.
- Drag across the arrangement ruler to define a loop range, then enable Loop.
  Pattern playback can audition the selected pattern through each instrument's
  default mixer lane without requiring an arrangement clip.
- Synth and effect controls use rotary knobs. Alt-click resets to the control's
  default, Ctrl-click opens numeric entry, and right-click opens value and
  automation actions. Delay and M1 decay expose step or millisecond timing.
- Selectable notes, instruments, clips, and mixer channels, plus reusable
  context menus with actions, separators, and nested action folders.
- SVG-backed Pen, Brush, Join, Cut, delete, pan, note-edge, and clip-edge
  cursor feedback; piano-roll note starts and ends can both be resized.
- Workflow shortcuts: Space (play/stop), F6 (channel rack), F7 (piano roll),
  F8 (MIDI routing), F9 (mixer), Shift+F9 (routing), F10 (automation), Ctrl+B
  or Ctrl+D (duplicate selected clip), Delete (delete selected clip), L (toggle
  loop), Ctrl+Shift+P (selected-pattern playback), Ctrl+S/Ctrl+O (save/load),
  Home (reset editor panning), Escape (clear selection), and P/B/J/C for
  piano-roll tools. Shift-click duplicates an arrangement clip.
- Editable shortcut reference/template at [shortcuts.json](./shortcuts.json).
- JSON project save/load.

VST3 and CLAP bundles are genuinely hosted (see [Plug-ins](#plug-ins)): their
DSP runs in the mixer, their editors attach as native child windows inside the
plug-in window, and their parameters are exposed as knobs. Plug-in state,
preset files and crash isolation (`vst3-host`'s process isolation) are not wired
up yet, and VST 2 remains discovery-only.

## Build and run

```text
cargo run
```

The app uses the system's default audio output and MIDI input. If audio cannot
be opened, the app reports the device error instead of silently claiming
playback is active. Samples are loaded from WAV paths in the channel rack or
audio-clip controls. Projects are JSON files and store sample paths rather than
copied audio.

Linux builds may need the platform development packages required by CPAL and
eframe (including ALSA and the selected X11/Wayland graphics stack).

### Command line

Every switch is optional; with no arguments the app uses `settings.json` and
`session.json` in the working directory.

```text
pulseforge [-s|--settings FILE] [-p|--project FILE] [-d|--debug] [--no-audio]
```

| Option | Effect |
| --- | --- |
| `-s`, `--settings <FILE>` | Settings file to read at start-up and write when saving |
| `-p`, `--project <FILE>` | Project file loaded at start-up; saves return to this path |
| `-d`, `--debug` | Open the debug/profiler panel immediately |
| `--no-audio` | Start without opening the audio device; hosted plug-ins stay unloaded |

`--help` and `--version` are provided by `clap`. Helper scripts in
`ai_scripts/ps/` wrap the common invocations (`run.ps1`, `check.ps1`,
`test.ps1`, `doc.ps1`).

## Debugging and profiling

`pulseforge-profiling` is a separate crate with four layers:

- **Recorder** (`span`): `profile_scope!("name")` records a named, nested scope
  into a lock-free queue. Names are `&'static str`, so recording never
  allocates, and with profiling disabled a scope costs one atomic load — it is
  safe on the audio thread.
- **Aggregator** (`flame`): a worker thread drains the queue every 80 ms and
  folds the last few seconds into a call tree per channel (`Ui`, `Audio`,
  `Worker`, `Other`), merging threads by name and computing exclusive time.
- **System sampler** (`system`): CPU, machine RAM and process RSS from
  `sysinfo` on its own thread, refreshed every 500 ms.
- **Debug panel** (`ui`): Ctrl+Shift+D or `--debug`. It shows UI frame time,
  FPS, worst frame, audio DSP load and peak, two overview strips, the flame
  graph with inclusive/exclusive hover details, and a table of hosted plug-ins.

Audio numbers come from the engine itself: the output callback publishes its
real-time load (work versus buffer period) and block size, and every hosted
VST3/CLAP slot records per-block time, peak time and the last error using plain
atomics — nothing on the audio path locks.

The overview strips reduce history to one min/max/mean column per two pixels.
When the driver offers GL 4.3 that reduction runs in a compute shader
(`gpu` feature); otherwise an identical CPU path is used and the panel says so.

### Debugging plug-ins

- Load failures are recorded per slot and shown in the panel next to the
  plug-in that produced them.
- The plug-in table ranks slots by CPU, so a hot synth or a runaway reverb is
  obvious within a second.
- `--no-audio` starts the UI without instantiating any plug-in, which is the
  safe way to open a project that crashes while loading one.
- Background work (folder scans, WAV decoding, project IO) runs on worker
  threads and appears under the `Worker` channel in the flame graph.

## Plug-ins

The plug-in layer is its own crate, `pulseforge-plugins`:

- `builtin` holds the DSP for the included processors.
- `registry` lists generators and effects so the picker and engine share one
  source of truth.
- `external` implements VST 2, VST3 and CLAP discovery plus the `PluginHost`
  boundary.

### VST3 and CLAP hosting

A second crate, `pulseforge-hosting`, owns the real hosting. It wraps two
battle-tested crates ? `vst3-host` for VST3 and `clack-host` for CLAP ? behind
one small API, so the DAW never depends on either format directly.

A load returns two halves, because the formats disagree on threading:

- `HostedUi` stays on the UI thread and owns the editor, the parameter table and
  the bundle metadata.
- `HostedAudio` is `Send` and owns the audio side, so the real-time engine can
  render with it. VST3 shares one `Plugin` behind a mutex (the same pattern
  `vst3-host`'s own helpers use); CLAP hands out a `Send`
  `StartedPluginAudioProcessor` while the `!Send` `PluginInstance` stays behind.

`pulseforge-core::hosted::HostedSlot` bridges the block/sample mismatch: the
engine renders one sample at a time, so each slot accumulates a 64-frame block,
processes it, and replays the processed block one sample later. That costs
exactly one block of latency per plug-in (?1.3 ms at 48 kHz) and allocates
nothing in the callback.

Editors attach as native child windows: `pulseforge_hosting::editor::EditorWindow`
creates a `WS_CHILD` window under the DAW window, and its rectangle is kept in
sync with the egui wrapper every frame. Because it is a real OS window it always
paints above the egui surface, so it is hidden when its plug-in window closes.

Choosing an external generator switches the instrument to `SynthKind::External`
and hands the note stream to the hosted instance, which allocates no built-in
voice; inserted bundles process the mixer lane in place. `restore_hosted_plugins`
re-instantiates every referenced bundle after a project load, because instances
are never serialized.

The wrapper window shows only the bundle identity and, once the plug-in's own
editor has changed a parameter, an **Automate** button that creates an
automation lane for it. Plug-in parameters are never mirrored as a second set of
knobs above the editor: the plug-in draws its own controls.

Editors are resizable. The wrapper asks the plug-in for the size it accepted on
attach, then forwards every window resize through `HostedUi::resize_editor`, so
VST3 `IPlugView::setFrame` and CLAP `gui.set_size` stay in step with the window.

Plug-in state is saved with the project: `capture_plugin_states` asks each
instance for its opaque blob before serializing, and `stored_plugin_state`
restores it right after the bundle is reloaded. `HostedUi::save_preset` writes a
`.vstpreset` for VST3 and the raw state for CLAP. Crash isolation is available in
`vst3-host` but is not enabled, because the isolated path cannot host an
embedded editor.

`pulseforge-core` re-exports the plug-in types, so call sites keep a single
import path.

## Performance model

The audio callback is allocation-free in steady state and no longer evaluates
automation per sample:

- Instrument controls and insert-effect parameters are refreshed once per
  16-frame control block instead of every frame, which cut automation cost by
  roughly 8x on a dense 24-lane test project (measured in release mode).
- Enabled automation lanes are indexed by target when the project changes, so a
  parameter lookup only walks lanes that actually drive it.
- Sends are cached as sparse edges and only rebuilt when routing changes.
- Project snapshots are coalesced on the UI thread (at most one every 20 ms) and
  the callback keeps only the newest one; snapshots that do not change instrument
  samples or clip audio paths reuse the already-loaded sample buffers, and the
  replaced snapshot is handed back to the UI thread for deallocation.
- The output stream requests a 256-frame buffer for lower monitoring latency,
  falling back to the device default if the backend refuses that size.

The UI reads a fractional playhead published by the audio thread and
extrapolates it between callbacks, so the playhead animates at the display
refresh rate (8 ms repaint budget while playing) rather than stepping once per
audio buffer.

## Feedback routing

Mixer edges are processed with a one-sample delay, so cyclic routes do not
create a same-frame dependency. Before applying the routing matrix, the engine
limits the sum of absolute incoming send gains per destination to 0.95. This
keeps the feedback network bounded under the current linear routing model;
the master output also has a hard peak limit.

## Controls

The arrangement remains visible as the main workspace. Its playhead line tracks
the current playback step. Set the global snap from the toolbar; it controls
arrangement clips, loop bounds, piano notes, and automation points. The
arrangement clips, loops, and automation points remain whole-step based while
piano notes support sub-step positions. Use the toolbar or
shortcuts to open the movable editors. Select a clip and press Ctrl+B to place
a linked duplicate immediately after it; right-click a pattern clip and choose
Pattern > Make unique to give that copy independent pattern data. Channel-rack
left-drag paints steps on;
right-drag paints them off. Mixer routing is a separate window: drag a source
node to a destination, hover a cable and drag its knob vertically to change
send gain, right-click a cable to remove it, or right-click a node to clear all
its incoming and outgoing sends. Right-click a mixer channel title to pin it or
open its channel actions. Compact mixer strips keep level, pan, and meter
visible while the right-side Controls/Inserts inspector edits the selected
channel.

The piano-roll toolbar stays pinned while the note grid scrolls, and the grid
has a step ruler across the top. Stopping playback releases every ringing
voice instead of letting notes sustain.

Piano-roll shortcuts P/B/J/C select Pen/Brush/Join/Cut. The clip arrangement
shows the same tool strip without Join, so Pen, Brush, and Cut behave the same
in both editors; Alt bypasses snap in either one. Right-drag deletes notes and
middle-drag pans. Drag either visible note edge to resize it; the arrangement
clip's right edge can likewise be dragged to change its length. Drag in the
velocity strip under the piano grid to change how hard a note is played.
Plug-in editors and their selector are independent, resizable windows. The
automation editor lets you select an instrument or mixer insert parameter,
create a lane, and choose a curve, MIDI envelope, LFO, or Perlin source.
Right-click a point to select its incoming curve shape or delete it. Draw a
multiplier curve over an envelope, LFO, or Perlin lane. Right-click a parameter
knob to create a lane and open the automation editor preselected for that
parameter. Drag above the arrangement lanes to mark a loop range. Ctrl+Z undoes
and Ctrl+Y or Ctrl+Shift+Z redoes the last structural edit.

Track headers respond to a double-click or F2 with the rename prompt, and their
context menu also reorders the track, collapses its automation rows, and adds a
new automation clip for that mixer channel. Drag a track's bottom edge to change
its height. Automation clips drag left and right just like pattern clips, and
duplicating one places the copy immediately after the original.

The keyboard map is editable in Settings ? Shortcuts: click a chord, press the
keys you want, and it is stored in `settings.json`. Bindings are grouped by
function and every group collapses. Clicking a surface no longer passes the
click through to a window behind it.

Every enable switch follows the same rule everywhere (arrangement headers,
channel rack, mixer strips and insert slots): click toggles the switch, Shift
locks it, Ctrl solos it. The whole mixer strip is clickable to select its
channel, and Shift/Ctrl extend the selection. A soloed insert silences the other
inserts on its channel.

Knobs are relative controls: Alt resets to the default, Ctrl+Click opens a value
entry, right-click opens the parameter menu, and while a drag is in progress the
pointer is hidden and returned to where it started on release.

The piano roll has a scale selector, a root selector, and a Snap-to-scale
toggle. Rows outside the scale are dimmed; with snapping on, drawn and moved
notes land on the nearest scale tone. Shift+Up/Down moves the selected notes (or
the whole pattern when nothing is selected) by a semitone, Ctrl+Shift+Up/Down by
an octave, and Shift+Left/Right by the current snap size.

Audio clips are decoded once per file path and cached by the engine, so a clip
keeps its audio across project rebuilds regardless of the order edits arrive in.
Double-click an audio clip to open its sample window: file name, frame count,
duration, waveform, and both the project-wide and the per-clip volume, pan and
pitch controls side by side.

Theming lives in Settings ? Theming. A theme only changes appearance: accent,
background, panel, MIDI-note and clip colours, the three meter colours, text,
font scale, and an optional background-image path, plus four presets. Settings ?
Plug-ins now also stores each hosted plug-in's opaque state in the project and
restores it on load.

Loop ranges are always at least one step long, and a click without a drag keeps
the previous loop instead of creating a one-step one. Dragging across the
piano-roll ruler selects every note that starts inside the range; it never
changes the transport loop.

The supplied 60-minute beginner tutorial was reviewed with one sampled frame
per minute and its English automatic-caption transcript. It covers the
Browser-to-channel-rack workflow, step and piano-roll editing, pattern/song
playback, linked pattern copies and making them unique, sample-clip repetition,
mixer inserts, automation, recording, and rendering. The samples are not a
literal review of every encoded frame. PulseForge uses the editing insights
that fit its scope. A follow-up transcript audit reinforced the value of
separate instrument/effect windows, sample-player workflows, editable clip and
note boundaries, and a visible tool cursor; those are now represented in the
prototype. Tempo-synced sample stretching/pitch controls and audio/MIDI
recording remain future work. WAV rendering is available from the Render WAV
window. The workspace is split into `pulseforge-plugins` (built-in DSP, plug-in
registry, and external plug-in discovery), `pulseforge-hosting` (real VST3 and
CLAP hosting plus the native editor window), `pulseforge-core` (project model,
audio engine, and parameter smoothing) and the root crate (egui interface and
platform integration). The interface, plug-ins, and assets remain original
rather than copied from the reference.

## License

No license has been selected for this project yet.
