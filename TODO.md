# TODO.md

A file to keep track of TODOs and unimplemented features.

# Engine

# UI

- Fix misaligned items.
- Add shortcuts for file operations and every operation mentioned here.

## Mixer

- Add an effect disable knob to the mixer channel strip, which temporarily avoids FX. It should not change their enable states.
  *Done: the strip's "FX bypass" switch rebuilds the channel's chain empty without touching any insert state.*
  Not fixed.
- Add the option to have multiple level meters. They should show waveform going from top to bottom, LUFs, Peak values, stereo correlation, etc. Options should be available in the graphics options.
- Add an insert effect which exposes a variable number of knobs and selectors. If it is present, show its controls above the inserts. Its UI selectors should be able to be mapped to a variable number of parameters on the mixer track using the mathematical formulas mentioned.
- Add a peak controller, LFO and perlin noise insert plugin which allows automating parameters.


## Automation clips

- Create an LFO which is keytracked to a MIDI track. Allow for changing the octave of the keytrack by +-10.
- Automation should allow optionally using formulas.

## Channel rack and Step sequencer

- Add support for rendering instruments. It should create a new track. The original should be grouped below it. Rendering should allow for sampling a range of notes from A to B every X notes Y times. It should create a soundfont instrument from that.
- Add controls for fine pitch, pan, velocity and repeats for each step.
- When middle clicking a step, it should fill the rest of the pattern with the steps up to that point. It should support polyrhythms.

## Piano roll

- Add controls for fine pitch and pan for each note.

## Arrangement

- Allow rendering clips in place. It should then mute and lock the original track, grouping it below the output. Apply mixer effects and everything, but play only the selected pattern as well as automation clips.
- Allow reordering by dragging.

## Automation

- Allow for smoothing parameters linearly.

## Icons

- Split the tool_cursors and other icons into neatly foldered svg icons. Allow themes to override them.