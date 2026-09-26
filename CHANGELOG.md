# Changelog

## 0.3.3

- Present Freqs as an MTS-ESP tuning system throughout the plug-in and documentation; describe its built-in sound as a tuning audition aid.
- Remove the synth subcategory from VST3 host metadata while keeping MIDI input and stereo audition output.

## 0.3.2

- Add subtle, flowing contour lines to the scale landscape, shaped by the positions of the current variations.
- Add a guarded Developer ID signing, notarization, stapling, and ZIP audit path for distributable VST3 packages.

## 0.3.1

- Remove the center cross from the scale landscape, the row and column gridlines from the note lattice, and the vertical guides from the pitch line.

## 0.3.0

- Rebuild the native editor around the reference design: dark bordered panels, coral section tabs, a circular note lattice, a dotted scale landscape, a labelled pitch line, and compact tuning, scale, synth, and stats controls in one view.
- Put less guidance in the main surface and provide usage help behind the help button. Keep Equal, Just major, and Snap note in the settings menu.
- Add a repeat-size snap toggle and a 7-note quick choice. Retain all scale, MIDI, MTS-ESP, and synth behavior.
- Add screenshot coverage for a played note and a 48-note scale.

## 0.2.1

- Pitch-line clicks and drags retune silently. MIDI notes highlight the matching scale degree while held, with a brief softer mark after release.

## 0.2.0

- Register as a VST3 instrument with MIDI input and stereo synth output. Sixteen voices offer triangle, square, and saw tones with level, attack, and release controls.
- Map consecutive MIDI keys to consecutive scale notes, with 3–48 notes per repeating period and quick 12/24/36/48 choices.
- Put variation reach beside the landscape and group root pitch and period size under tuning. Keep all views and audition controls in one window.
- Preserve MTS-ESP master tuning and local scale slots; read earlier scale files and host states.

## 0.1.1

- Initial macOS VST3 MTS-ESP master with native Toybox/GPUI editor and stereo pass-through.
- Pitch line, live pair-fit matrix, scale variation field, root and period controls, and scale metrics.
- VST3 project state and eight local save/load slots.
