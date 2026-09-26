# Freqs project

Freqs is a native Toybox/GPUI VST3 synth instrument and MTS-ESP master for designing repeating microtonal scales. It is an independent AudioDev plug-in repository. The processor takes MIDI input and renders 16-voice stereo synth audio, while a worker thread publishes a 128-key tuning table to the system MTS-ESP library. Host project state and eight local saved-scale slots are separate persistence paths.

The editor shows a pitch line, circular note lattice, whole-scale variation field, tuning controls, audition synth, and scale stats together in one fixed window. It follows the Pump/Wavecrate/Sift charcoal and coral visual palette. Keep host and GUI mechanics in Toybox where possible; Freqs owns only its tuning, analysis, and product-specific editor behavior.
