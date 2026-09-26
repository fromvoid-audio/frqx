//! Shared non-audio tuning state and MTS-ESP connection status.

use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

use crate::scale::Scale;
use crate::synth::{
    SynthSettings, Waveform, ATTACK_PARAM, LEVEL_PARAM, RELEASE_PARAM, WAVEFORM_PARAM,
};

/// Current MTS-ESP master connection status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MasterStatus {
    /// The worker is loading the MTS-ESP library.
    Connecting = 0,
    /// This plug-in owns the master connection.
    Active = 1,
    /// Another plug-in is currently the master.
    Blocked = 2,
    /// The installed MTS-ESP library is unavailable or incompatible.
    Unavailable = 3,
}

/// State shared by the processor and GPUI editor.
pub struct Shared {
    scale: Mutex<Scale>,
    revision: AtomicU64,
    status: AtomicU8,
    clients: AtomicI32,
    frequencies: [AtomicU64; 128],
    waveform: AtomicU8,
    level: AtomicU32,
    attack_ms: AtomicU32,
    release_ms: AtomicU32,
    held_keys: [AtomicU64; 2],
    last_played_key: AtomicI32,
    last_played_revision: AtomicU64,
}

impl Default for Shared {
    fn default() -> Self {
        let defaults = SynthSettings::default();
        let frequencies = Scale::default().frequencies();
        Self {
            scale: Mutex::new(Scale::default()),
            revision: AtomicU64::new(1),
            status: AtomicU8::new(MasterStatus::Connecting as u8),
            clients: AtomicI32::new(0),
            frequencies: std::array::from_fn(|key| AtomicU64::new(frequencies[key].to_bits())),
            waveform: AtomicU8::new(defaults.waveform.index()),
            level: AtomicU32::new(defaults.level.to_bits()),
            attack_ms: AtomicU32::new(defaults.attack_ms.to_bits()),
            release_ms: AtomicU32::new(defaults.release_ms.to_bits()),
            held_keys: std::array::from_fn(|_| AtomicU64::new(0)),
            last_played_key: AtomicI32::new(-1),
            last_played_revision: AtomicU64::new(0),
        }
    }
}

impl Shared {
    /// Read a consistent copy of the tuning without involving the audio thread.
    pub fn snapshot(&self) -> Scale {
        *self
            .scale
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Change the tuning and notify the MTS worker if its value changed.
    pub fn edit(&self, edit: impl FnOnce(&mut Scale)) {
        let mut state = self
            .scale
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let before = *state;
        edit(&mut state);
        if *state != before {
            let frequencies = state.frequencies();
            for (slot, frequency) in self.frequencies.iter().zip(frequencies) {
                slot.store(frequency.to_bits(), Ordering::Release);
            }
            self.revision.fetch_add(1, Ordering::Release);
        }
    }

    /// Replace the tuning after validation, as when loading host state.
    pub fn replace(&self, next: Scale) -> bool {
        if !next.valid() {
            return false;
        }
        self.edit(|scale| *scale = next);
        true
    }

    /// Return the most recent tuning revision.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Read one MIDI key's tuned frequency without taking an audio-thread lock.
    pub fn frequency(&self, key: u8) -> f64 {
        f64::from_bits(self.frequencies[usize::from(key)].load(Ordering::Acquire))
    }

    /// Read audition settings without taking an audio-thread lock.
    pub fn synth_settings(&self) -> SynthSettings {
        SynthSettings {
            waveform: Waveform::from_index(self.waveform.load(Ordering::Acquire)),
            level: f32::from_bits(self.level.load(Ordering::Acquire)),
            attack_ms: f32::from_bits(self.attack_ms.load(Ordering::Acquire)),
            release_ms: f32::from_bits(self.release_ms.load(Ordering::Acquire)),
        }
    }

    /// Replace valid audition settings, as when a host project is loaded.
    pub fn set_synth_settings(&self, settings: SynthSettings) -> bool {
        if !settings.valid() {
            return false;
        }
        self.waveform
            .store(settings.waveform.index(), Ordering::Release);
        self.level
            .store(settings.level.to_bits(), Ordering::Release);
        self.attack_ms
            .store(settings.attack_ms.to_bits(), Ordering::Release);
        self.release_ms
            .store(settings.release_ms.to_bits(), Ordering::Release);
        true
    }

    /// Apply one host-normalized audition parameter.
    pub fn set_synth_param(&self, id: u32, normalized: f64) -> bool {
        if !normalized.is_finite() || !(0.0..=1.0).contains(&normalized) {
            return false;
        }
        match id {
            WAVEFORM_PARAM => self
                .waveform
                .store((normalized * 2.0).round() as u8, Ordering::Release),
            LEVEL_PARAM => self
                .level
                .store((normalized as f32).to_bits(), Ordering::Release),
            ATTACK_PARAM => self.attack_ms.store(
                (2.0 + normalized as f32 * 498.0).to_bits(),
                Ordering::Release,
            ),
            RELEASE_PARAM => self.release_ms.store(
                (100.0 + normalized as f32 * 2900.0).to_bits(),
                Ordering::Release,
            ),
            _ => return false,
        }
        true
    }

    /// Read one host-normalized audition parameter.
    pub fn synth_param(&self, id: u32) -> Option<f64> {
        let settings = self.synth_settings();
        match id {
            WAVEFORM_PARAM => Some(f64::from(settings.waveform.index()) / 2.0),
            LEVEL_PARAM => Some(f64::from(settings.level)),
            ATTACK_PARAM => Some(f64::from((settings.attack_ms - 2.0) / 498.0)),
            RELEASE_PARAM => Some(f64::from((settings.release_ms - 100.0) / 2900.0)),
            _ => None,
        }
    }

    /// Publish MIDI keys currently held by the synth.
    pub fn set_held_keys(&self, keys: [u64; 2]) {
        for (slot, bits) in self.held_keys.iter().zip(keys) {
            slot.store(bits, Ordering::Release);
        }
    }

    /// Read the currently held MIDI keys for the editor.
    pub fn held_keys(&self) -> [u64; 2] {
        std::array::from_fn(|index| self.held_keys[index].load(Ordering::Acquire))
    }

    /// Record a MIDI note-on for editor selection and feedback.
    pub fn note_played(&self, key: u8) {
        self.last_played_key
            .store(i32::from(key), Ordering::Release);
        self.last_played_revision.fetch_add(1, Ordering::Release);
    }

    /// Return the latest note-on key and its revision.
    pub fn last_played(&self) -> (u64, i32) {
        (
            self.last_played_revision.load(Ordering::Acquire),
            self.last_played_key.load(Ordering::Acquire),
        )
    }

    /// Read the current master state.
    pub fn status(&self) -> MasterStatus {
        match self.status.load(Ordering::Acquire) {
            1 => MasterStatus::Active,
            2 => MasterStatus::Blocked,
            3 => MasterStatus::Unavailable,
            _ => MasterStatus::Connecting,
        }
    }

    /// Read the connected client count reported by MTS-ESP.
    pub fn clients(&self) -> i32 {
        self.clients.load(Ordering::Acquire)
    }

    #[cfg(feature = "vst3")]
    pub(crate) fn set_status(&self, status: MasterStatus) {
        self.status.store(status as u8, Ordering::Release);
    }

    #[cfg(feature = "vst3")]
    pub(crate) fn set_clients(&self, clients: i32) {
        self.clients.store(clients, Ordering::Release);
    }
}
