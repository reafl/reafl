# TODO.md

A file to keep track of TODOs and unimplemented features.

# Plugins

- Reformat code to be modular. Plugins should be easy to add.

# Engine

- Create a way to quickly evaluate user-inputted mathematic formulas. They should support +-/*%^ exp sqrt sin cos tan.
  *Done: `reafl_core::expr` parses and evaluates formulas (`+ - * / % ^`, `exp ln sqrt abs floor round sin cos tan`, `pi`, `e`, variable `x`), and the automation window turns one into a sampled curve lane.*

# UI

- There should be a DPI-like setting for the UI.
  *Done: Settings → Graphics → Interface scale (0.6×–2.5×), persisted as `ui_scale`.*
- Remove the help button.
  *Done: removed from the toolbar; the full key map lives in Settings → Shortcuts.*
- Remove the zoom control.
  *Done: removed from the toolbar; zoom stays on Ctrl+wheel and in Settings → Graphics.*
- Fix misaligned items.
- Allow zooming in further. Ctrl+Click should reset zoom.
  *Done: range is now 0.2×–8×, and Ctrl+click on the arrangement ruler resets to 100 %.*
  Note: Zoom in should be capped at even lower values.
  *Done: the upper limit is now 3× (`MAX_ZOOM`/normalize).*
- Add shortcuts for file operations and every operation mentioned here.
- Right clicking (entering delete mode) should unselect everything in piano roll and arrangement window.
  *Done: a secondary press clears the selection in both editors.*

## Mixer

- The visual for the mixer value starts at 0.
  *Done: the fader draws a filled dB column with its handle, instead of a slider whose track starts at zero.*
- Mixer levels should go down to 100dB, after which they snap to 0.
  *Done: the strip fader is a dB scale down to −100 dB and its bottom position is exact silence.*
- There should be up to 10 inserts available.
  *Done: `INSERT_SLOTS = 10`; the strip's insert list scrolls and the mixer header adds channels.*
- Pan and level controls should disappear from the selected track window.
  *Done: the inspector shows EQ, width and inserts only; level and pan live on the strips.*
- Add a stereo width knob to the mixer channel strip.
  *Done: `ArrangementLane::width` drives a mid/side scale in the engine and a compact knob next to pan.*
- Add an effect disable knob to the mixer channel strip, which temporarily avoids FX. It should not change their enable states.
  *Done: the strip's "FX bypass" switch rebuilds the channel's chain empty without touching any insert state.*
  Not fixed.
- Add the option to have multiple level meters. They should show waveform going from top to bottom, LUFs, Peak values, stereo correlation, etc. Options should be available in the graphics options.
- Join the selected track controls and inserts tabs.
  *Done: one inspector page with EQ, stereo width and the insert chain.*
- Allow saving mixer chains as presets.
  *Done: "Save chain" / "Load chain" in the mixer inspector, stored as JSON in `presets/mixer/`, plus the right-click preset menu on "+ Track".*
- Add an insert effect which exposes a variable number of knobs and selectors. If it is present, show its controls above the inserts. Its UI selectors should be able to be mapped to a variable number of parameters on the mixer track using the mathematical formulas mentioned.
- Add an add track button right of all mixer tracks. It should add an empty track on left click and open a preset menu on right click.
  *Done: the "+ Track" button at the end of the strip row adds an empty channel on left click and lists the saved mixer chain presets on right click.*
- Add a peak controller, LFO and perlin noise insert plugin which allows automating parameters.


## Automation clips

- Move sample and hold to its own category.
  *Done: `AutomationSource::SampleAndHold { seed, rate, base, depth }` is a source of its own in the picker, engine and preview; the old `LfoShape::SampleAndHold` still loads for existing projects but is no longer offered.*
- Create an LFO which is keytracked to a MIDI track. Allow for changing the octave of the keytrack by +-10.
- Automation should allow for relative automation.
  *Done: `AutomationSource::RelativeCurve` draws offsets that are added to the parameter's own value, so several relative lanes can stack over an absolute one.*
- The automation should be allowed to be negative.
  *Done: relative lanes span -1 to 1 around a zero line and survive normalization; absolute lanes stay inside the parameter's 0-1 range.*
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