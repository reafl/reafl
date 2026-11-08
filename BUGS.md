
# BUGS.md

This file is a bugtracker.

# UI

## Tools


## Top bar


## Arrangement

- There is clickthrough issues when selecting a loop or clicking below the tracks.
- Enable buttons are not colored here.
- Shift clicking a clip should duplicate it. When dragging, it should follow the mouse cursor. If not, append it at the nearest free spot.
  *Partly fixed: shift+click now duplicates the clip (and selects the copy). Dragging the fresh copy mid-gesture is still queued.*
- Middle click panning should not snap. Automation clips render outside of the left bounds.
  *Partly fixed: automation clips (and their curves and points) are clipped to the grid instead of painting over the track header. Panning still snaps to steps.*

## Mixer

- Mixer channel height should adjust to the window.

## Mixer routing




## Piano roll

- The brush tool does not brush notes but draws one continious one.
- The old repeat UI is still visible.
- Repeats do not give a visual indicator. There should be lines on the note showing where said note repeats.
  *Fixed: repeat boundaries are drawn as dividers inside the note.*
  Not fixed. Nothing appears.
- Shifting notes (shift+up/down/left/right) does not work.
- Short notes have a wrong selection hitbox.
- Middle click to pan does not work. UPDATE: It does work but snaps.

## Enables


## Automation Clips

- There is jittering when zooming in/out on midi envelopes. There should be more sample points available graphically.
  *Fixed: envelopes are sampled on a fixed quarter-step grid instead of per pixel column, so zooming no longer shifts the samples.*
  Note: Add a multiplier, tweakable from settings, for substep sampling.
- MIDI envelopes do not render.

## Step sequencer

## Plugin windows


