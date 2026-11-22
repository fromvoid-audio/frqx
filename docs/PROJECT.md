# frqx project

frqx is a native Toybox/GPUI MTS-ESP tuning system for designing repeating microtonal scales. It is an independent AudioDev plug-in repository. A worker thread publishes a 128-key tuning table to the system MTS-ESP library. The VST3 accepts MIDI and produces a simple 16-voice reference tone for auditioning the tuning; this sound is a tuning aid rather than the product's primary purpose. Host project state and eight local saved-scale slots are separate persistence paths.

The editor shows a pitch line, circular note lattice, whole-scale variation field, tuning controls, audition tone controls, and scale stats together in one fixed window. It follows the Pump/Wavecrate/Sift charcoal and coral visual palette. Keep host and GUI mechanics in Toybox where possible; frqx owns only its tuning, analysis, and product-specific editor behavior.
