# Freqs

Freqs is a macOS VST3 synth instrument, scale designer, and MTS-ESP master built with Toybox and GPUI. MIDI notes play its 16-voice audition synth through a stereo output. It also publishes a 128-key tuning table to MTS-ESP clients.

## Editor

- **Scale Landscape** shows frozen scale variations positioned by whole-scale harmonic cohesion and root affinity. Click a point to select a variation. **New Variations** creates a new field from the current scale. The Reach slider updates the field while dragging.
- **Note Lattice** shows harmonic simplicity for every note pair. Drag a dot vertically to increase or decrease the selected pair's fit. Every dot updates during the drag.
- **Pitch Line** lets you drag individual degrees silently. Held MIDI notes highlight the corresponding scale degrees, with a brief mark after release. All main views stay visible together. Equal, Just major, and Snap selected note live under the settings icon.
- **Tuning** groups root frequency and repeat size. Hold Shift while dragging the period slider, or enable Snap Ratios, to lock to clean ratios. Root degree and root MIDI key are separate controls. **Reach** sits with the landscape it changes.
- **Audition Synth** offers triangle, square, and saw tones, plus output level, attack, and release controls. Play MIDI to hear the scale.
- **Notes / Repeat** sets 3–48 notes. Use ±1 or the 12, 24, 36, and 48 shortcuts. Each adjacent MIDI key plays the next scale note, wrapping to the next period after the last note.
- The editor reports scale cohesion, root affinity, spectral interaction, and a strong three-note root shape. These are visual estimates, not musical verdicts.
- **Save** and **Load** use eight local scale slots under `~/Library/Application Support/PORTALSURFER/Freqs/scales/`. The VST3 also stores its scale and synth settings in the host project state. Earlier scale files and host states remain readable.

## Host test

1. Disable any other MTS-ESP master. Only one master can own the connection.
2. Place the built `.vst3` bundle in `~/Library/Audio/Plug-Ins/VST3/`, rescan plug-ins, and load Freqs as an instrument in the DAW.
3. Play MIDI into Freqs and verify its stereo synth output, release tail, and keyboard mapping at 12, 24, and 48 notes. Tune a degree while holding a note to hear the pitch change.
4. Optionally load an MTS-ESP compatible instrument and verify Freqs also tunes it. The header shows connection state and client count.
5. Save and reload a DAW project, then test the eight local library slots separately.

The default scale is seven-degree 12-TET major with MIDI key 60 as a 220 Hz root and a 1200-cent period. Freqs supports 3–48 degrees and periods from 600 to 2400 cents. MTS-ESP client behavior for notes already sounding depends on the instrument.

## Build

Run from this repository with the VST3 SDK path set:

```fish
set -gx VST3_SDK_DIR /Users/portalsurfer/dev/audiodev/vst3sdk
bash scripts/ci.sh
bash scripts/ci.sh --vst3
bash scripts/ci.sh --screenshots
bash scripts/dist.sh --format vst3
```

`CARGO_TARGET_DIR` may point at a shared local build cache. The distribution script creates an ad-hoc signed local test bundle. It does not install, notarize, or publish it.

## Release package

`scripts/release.sh` creates a separate distributable macOS VST3 ZIP. It requires a clean committed source tree, the VST3 SDK, an installed Developer ID Application certificate, and Apple notarization credentials. It signs with the hardened runtime and timestamp, waits for notarization, staples the ticket, then re-extracts and audits the ZIP. A failed gate produces no release package.

```fish
set -gx VST3_SDK_DIR /path/to/vst3sdk
set -gx APPLE_NOTARY_KEYCHAIN_PROFILE freqs-notary
bash scripts/release.sh --check
bash scripts/release.sh --package-only
```

`freqs-notary` must be a profile already saved in your macOS Keychain. To create one with an Apple ID, run `xcrun notarytool store-credentials freqs-notary --apple-id your-apple-id@example.com --team-id DKTKQ8U5T8`; it prompts securely for an app-specific password. Alternatively, supply `APPLE_NOTARY_KEY_PATH`, `APPLE_NOTARY_KEY_ID`, and `APPLE_NOTARY_ISSUER_ID` instead of a keychain profile. `APPLE_CODESIGN_IDENTITY` can select a specific installed Developer ID Application certificate. Credentials stay outside this repository. Releases are written under `dist/releases/freqs-v<version>-<source-sha>/` with a ZIP, install note, changelog, and hash/signing evidence. This script does not upload or publish; host testing and a separate publication decision follow package verification.
