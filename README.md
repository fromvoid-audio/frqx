# Freqs

Freqs is an MTS-ESP-based tuning system for designing, auditioning, and saving microtonal scales. It runs as an MTS-ESP master and sends a 128-key tuning table to compatible instruments. Its built-in sound is a simple reference tone for hearing the tuning as you edit; the plug-in accepts MIDI and produces stereo audio so you can audition scales directly in your DAW.

Freqs replaces an ODDSound master plug-in as the tuning controller. MTS-ESP still needs ODDSound's shared `libMTS.dylib` at `/Library/Application Support/MTS-ESP/`, because client instruments load that same library. The complete release ZIP includes ODDSound's signed library installer; users do not need to download or run a separate master plug-in. If the library is already installed, they can skip that installer.

## Editor

- **Nodes** lists each degree's absolute pitch in hertz and its position in cents. Press Enter to apply a field edit; Escape or leaving the field cancels the draft. Up/Down adjusts the value, with Shift for smaller steps. Pitch remains relative to the selected root, and edits stay ordered between neighboring nodes. Scroll the list for larger scales. Right-click a node to insert a degree after it or remove it. The root node cannot be removed, and a scale always keeps at least three nodes.
- **Pitch Space** shows scale variations positioned by whole-scale cohesion and root affinity. Click a point to apply a variation. The dice button creates another field from the current scale. The Reach bar updates the field while dragging.
- **Pitch Line** lets you drag individual degrees silently. Note numbers appear above the line. Cent values appear below it for smaller scales and the selected degree in larger scales. Held MIDI notes highlight their scale degrees, with a brief mark after release.
- **Reference** groups root degree, root frequency, root MIDI key, repeat size, and ratio snapping. Hold Shift while dragging the repeat bar, or enable Snap Ratios, to lock it to clean ratios. Equal divisions, Just major, and Snap selected note are under settings.
- **Slots & structure** sets 3–48 notes with the count buttons and provides eight local save/load slots. Each adjacent MIDI key plays the next scale note, wrapping to the next period after the last note.
- **Audition** offers triangle, square, and saw tones, plus output level, attack, and release controls for checking the current tuning with MIDI.
- **Save** and **Load** use eight local scale slots under `~/Library/Application Support/PORTALSURFER/Freqs/scales/`. The VST3 also stores its scale and synth settings in the host project state. Earlier scale files and host states remain readable.

## Host test

1. Disable any other MTS-ESP master. Only one master can own the connection.
2. Place the built `.vst3` bundle in `~/Library/Audio/Plug-Ins/VST3/`, rescan plug-ins, and load Freqs as an instrument in the DAW.
3. Play MIDI into Freqs and verify its reference tone, release tail, and keyboard mapping at 12, 24, and 48 notes. Tune a degree while holding a note to hear the pitch change.
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

`scripts/release.sh` creates a separate distributable macOS VST3 ZIP and a complete ZIP containing the VST3 plus ODDSound's signed MTS-ESP library installer. It requires a clean committed source tree, the VST3 SDK, an installed Developer ID Application certificate, and Apple notarization credentials. It signs Freqs with the hardened runtime and timestamp, waits for notarization, staples the ticket, then re-extracts and audits the ZIPs. It downloads the library installer from a pinned upstream revision and verifies its checksum and signature. A failed gate produces no release package.

```fish
set -gx VST3_SDK_DIR /path/to/vst3sdk
set -gx APPLE_NOTARY_KEYCHAIN_PROFILE freqs-notary
bash scripts/release.sh --check
bash scripts/release.sh --package-only
```

`freqs-notary` must be a profile already saved in your macOS Keychain. To create one with an Apple ID, run `xcrun notarytool store-credentials freqs-notary --apple-id your-apple-id@example.com --team-id DKTKQ8U5T8`; it prompts securely for an app-specific password. Alternatively, supply `APPLE_NOTARY_KEY_PATH`, `APPLE_NOTARY_KEY_ID`, and `APPLE_NOTARY_ISSUER_ID` instead of a keychain profile. `APPLE_CODESIGN_IDENTITY` can select a specific installed Developer ID Application certificate. Credentials stay outside this repository. Releases are written under `dist/releases/freqs-v<version>-<source-sha>/` with ZIPs, the signed ODDSound installer, install note, changelog, and hash/signing evidence. This script does not upload or publish; host testing and a separate publication decision follow package verification.
