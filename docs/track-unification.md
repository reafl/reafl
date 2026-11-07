# Track unification

The editor is moving from three parallel concepts to one. This file records the
target model and the order the work lands in, so each step can be reviewed and
reverted on its own.

## Problem

Today a project keeps three separate lists:

* `Project::instruments` — the channel rack (sound sources),
* `Project::lanes` — arrangement rows *and* mixer channels,
* `Project::patterns` — a shared note pool that clips reference by id.

They are joined by `Instrument::default_mixer_lane` and by `ClipSource::Pattern`,
so an instrument lives in one list, its mixer strip in another, and its notes in
a third. Automation has to address either an instrument, a mixer channel or a
clip, and there is no way to put two of them on one row.

## Target model

One `Track` is one row of the arrangement, one mixer strip and one rack slot:

```text
Track
├─ name, parent (group), colour, height, collapsed, enabled / locked / solo
├─ kind ─┬─ Generator(Generator)      // synth model, sample player or hosted plug-in
│        └─ Group                     // folder: no source of its own
├─ midi_fx   (transpose, velocity scale)       // applied before the source
├─ mixer     (gain, pan, width, EQ, inserts, send row)
├─ clips     (each clip owns its notes; audio clips own a path)
└─ group_targets (child parameters a folder additionally exposes)
```

Consequences:

* **Patterns disappear.** A clip carries its own notes, so `ClipSource`,
  `Pattern`, `StepHit` and `instrument_id` on notes all go away. The step
  sequencer edits the selected clip instead of a global pattern.
* **Groups** have no generator. They sum their children's audio (their own
  gain/pan/EQ/inserts process the sum) and pass notes down: a clip on a group
  plays through every descendant, after the group's `midi_fx` is applied.
* **Nesting is arbitrary**, with a cycle guard when a parent is assigned.
* **Old project files are not migrated.** `session.json` from the previous
  shape no longer loads; the app starts from a new default project.

## Automation targets on tracks

Automation addresses a track and one of its parameters:
`ParameterTarget::Track { track_id, parameter: TrackParameter }`, where

```text
TrackParameter
├─ Generator(InstrumentParameter)   // only on a source track whose model uses it
├─ MidiFx(MidiFxParameter)          // transpose, velocity scale
└─ Mixer(MixerParameter)            // gain, pan, width, EQ low/mid/high, sends
```

`Track::automation_parameters` is the one place that lists what a track offers:
generator controls the model actually uses (`Generator::supports`), plus every
MIDI FX and mixer control.

### Groups expose their children's common parameters

A folder track additionally lists parameters of its children, so one lane can
drive a whole submix:

* **Scope.** The comparison runs over *every* descendant, at any nesting depth.
  Nested folders are transparent: they are skipped rather than counted as
  "has no such parameter".
* **Always group-wide.** MIDI FX and mixer controls exist on every track, so
  they are group-wide without any user action (`TrackParameter::is_group_wide`).
  A value written on a group applies to the group's own strip and is inherited
  by every descendant.
* **Explicit picks.** Generator controls only exist on some tracks, so a group
  keeps them in `Track::group_targets`. `Track::available_group_targets` offers
  the parameters common to all descendants, and an entry stays active only while
  at least one descendant still supports it.
* **Precedence.** A descendant's own automation lane for a parameter always
  wins; the group value applies only where the descendant has no lane of its
  own. Base values are replaced, not offset.
* **Resolution order** for any (track, parameter) pair is therefore:
  1. the track's own lane,
  2. the nearest ancestor group's lane for that parameter,
  3. the track's base value.

Both the engine and the UI must call the same helper for step 2 and 3, otherwise
the drawn curves and the audio diverge (the bug fixed in `f8de37b`).

## Steps

Each step is its own commit (`AI: ...`) with the tests and the changelog entry
updated in the same commit, and the workspace must build and pass tests at every
one of them.

1. **Core types** — `project/model/track.rs` and `project/model/track_param.rs`:
   `TrackId`, `TrackKind`, `Generator`, `GeneratorKind`, `MidiFx`, `TrackMixer`,
   `Clip`, `ClipNote`, `MidiFxParameter`, `TrackParameter`, the tree helpers
   (`children`, `descendants`, `ancestors`, cycle guard) and the group-target
   helpers. Purely additive.
2. **Project owns tracks** — replace `instruments`, `lanes` and `patterns` with
   `tracks`; new default project (one group with two children); `normalize`
   validates parents, prunes dangling `group_targets`, clamps mixer values.
3. **Unified targets** — replace the instrument/mixer/clip target families with
   `ParameterTarget::Track` (+ clip and sample-default targets), one
   `parameter_value`, one base-value reader, one writer.
4. **Inheritance helper** — `effective_track_value(project, track, parameter)`:
   own lane, then nearest ancestor group lane, then base; used by the engine and
   by every UI read.
5. **Engine voices** — one voice pool per source track; group tracks are
   processed after their children so a group's chain runs on the sum.
6. **Engine notes** — clips on any track; a group's clip fans out to its
   descendants through `midi_fx`; audio clips keep their per-clip controls.
7. **UI track list** — the arrangement shows the track tree (indent, collapse),
   clips belong to their row, the rack and mixer become per-track views.
8. **Automation window** — the picker lists tracks instead of instrument/mixer/
   clip families, including a group's forwarded child parameters.
9. **Polish** — presets, docs, changelog, tests.

## Compatibility

Deliberately broken, per the decision recorded above: the project file format
changes shape, so the app resets rather than guessing. `serde` still uses
`#[serde(default)]` on the new structs, so a partially written track loads with
sensible values.
