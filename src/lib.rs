//! Freqs AudioDev plug-in.
//!
//! An MTS-ESP master with an editable microtonal scale and three GPUI views.

#![deny(missing_docs, warnings)]

pub mod analysis;
#[cfg(all(
    target_os = "macos",
    any(feature = "vst3", feature = "screenshot-test")
))]
pub mod gui_gpui;
pub mod library;
#[cfg(feature = "vst3")]
mod mts;
pub mod scale;
pub mod shared;
pub mod synth;

#[cfg(feature = "vst3")]
mod vst3;

#[cfg(feature = "vst3")]
toybox::vst3_plugin_entry!(vst3::Factory);
