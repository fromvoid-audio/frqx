//! frqx AudioDev plug-in.
//!
//! An MTS-ESP tuning system for designing and auditioning microtonal scales.

#![deny(missing_docs, warnings)]

/// Version displayed by the UI and VST3 factory, including local dev build labels.
pub const BUILD_VERSION: &str = match option_env!("PLUGIN_DEV_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

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
