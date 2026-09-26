//! Small polyphonic audition synthesizer for the current scale.

/// Maximum simultaneously sounding audition notes.
pub const MAX_VOICES: usize = 16;

/// Host-visible waveform parameter identifier.
pub const WAVEFORM_PARAM: u32 = 100;
/// Host-visible output level parameter identifier.
pub const LEVEL_PARAM: u32 = 101;
/// Host-visible attack parameter identifier.
pub const ATTACK_PARAM: u32 = 102;
/// Host-visible release parameter identifier.
pub const RELEASE_PARAM: u32 = 103;

/// Oscillator shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    /// Smoothest, with fewer upper harmonics.
    Triangle,
    /// Hollow odd-harmonic tone.
    Square,
    /// Bright full-harmonic tone.
    Saw,
}

impl Waveform {
    /// Convert a three-position selector index.
    pub fn from_index(index: u8) -> Self {
        match index {
            1 => Self::Square,
            2 => Self::Saw,
            _ => Self::Triangle,
        }
    }

    /// Return the selector index.
    pub fn index(self) -> u8 {
        match self {
            Self::Triangle => 0,
            Self::Square => 1,
            Self::Saw => 2,
        }
    }

    /// Short user-facing name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Triangle => "Triangle",
            Self::Square => "Square",
            Self::Saw => "Saw",
        }
    }
}

/// Audition synth settings shared between the UI and processor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SynthSettings {
    /// Oscillator shape.
    pub waveform: Waveform,
    /// Output level from zero to one.
    pub level: f32,
    /// Time to approach full level after a note-on, in milliseconds.
    pub attack_ms: f32,
    /// Time for a released note to decay to silence, in milliseconds.
    pub release_ms: f32,
}

impl Default for SynthSettings {
    fn default() -> Self {
        Self {
            waveform: Waveform::Triangle,
            level: 0.25,
            attack_ms: 10.0,
            release_ms: 900.0,
        }
    }
}

impl SynthSettings {
    /// Check values loaded from persisted state.
    pub fn valid(self) -> bool {
        self.level.is_finite()
            && (0.0..=1.0).contains(&self.level)
            && self.attack_ms.is_finite()
            && (2.0..=500.0).contains(&self.attack_ms)
            && self.release_ms.is_finite()
            && (100.0..=3000.0).contains(&self.release_ms)
    }
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    active: bool,
    held: bool,
    key: u8,
    channel: i16,
    note_id: i32,
    age: u64,
    phase: f64,
    frequency: f64,
    velocity: f32,
    envelope: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            held: false,
            key: 0,
            channel: 0,
            note_id: -1,
            age: 0,
            phase: 0.0,
            frequency: 440.0,
            velocity: 0.0,
            envelope: 0.0,
        }
    }
}

/// Fixed-capacity voice engine; all mutation happens on the audio callback.
pub struct Synth {
    voices: [Voice; MAX_VOICES],
    next_age: u64,
    sample_rate: f64,
    cached_attack_ms: f32,
    cached_release_ms: f32,
    attack_coefficient: f32,
    release_coefficient: f32,
}

impl Default for Synth {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl Synth {
    /// Construct an idle synth with a fallback sample rate.
    pub fn new(sample_rate: f64) -> Self {
        Self {
            voices: [Voice::default(); MAX_VOICES],
            next_age: 1,
            sample_rate: valid_rate(sample_rate),
            cached_attack_ms: f32::NAN,
            cached_release_ms: f32::NAN,
            attack_coefficient: 0.0,
            release_coefficient: 0.0,
        }
    }

    /// Follow a host sample-rate change without reallocating voices.
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        let next = valid_rate(sample_rate);
        if next != self.sample_rate {
            self.sample_rate = next;
            self.cached_attack_ms = f32::NAN;
            self.cached_release_ms = f32::NAN;
        }
    }

    /// Start one MIDI note.
    pub fn note_on(&mut self, key: u8, channel: i16, note_id: i32, velocity: f32, frequency: f64) {
        if velocity <= 0.0 || !frequency.is_finite() || frequency <= 0.0 {
            return;
        }
        let index = self
            .voices
            .iter()
            .position(|voice| !voice.active)
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .filter(|(_, voice)| !voice.held)
                    .min_by(|a, b| a.1.envelope.total_cmp(&b.1.envelope))
                    .map(|(index, _)| index)
            })
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, voice)| voice.age)
                    .map_or(0, |(index, _)| index)
            });
        self.voices[index] = Voice {
            active: true,
            held: true,
            key,
            channel,
            note_id,
            age: self.next_age,
            phase: 0.0,
            frequency,
            velocity: velocity.clamp(0.0, 1.0),
            envelope: 0.0,
        };
        self.next_age = self.next_age.wrapping_add(1);
    }

    /// Release matching MIDI notes, using note ID when available.
    pub fn note_off(&mut self, key: u8, channel: i16, note_id: i32) {
        for voice in &mut self.voices {
            if voice.active
                && voice.held
                && voice.channel == channel
                && if note_id >= 0 {
                    voice.note_id == note_id
                } else {
                    voice.key == key
                }
            {
                voice.held = false;
            }
        }
    }

    /// Release all notes, retaining their audible tails.
    pub fn release_all(&mut self) {
        for voice in &mut self.voices {
            voice.held = false;
        }
    }

    /// Silence all voices when the host restarts processing.
    pub fn clear(&mut self) {
        self.voices = [Voice::default(); MAX_VOICES];
    }

    /// Update held-note pitch after a scale edit.
    pub fn retune(&mut self, mut frequency: impl FnMut(u8) -> f64) {
        for voice in &mut self.voices {
            if voice.active {
                voice.frequency = frequency(voice.key);
            }
        }
    }

    /// Render one mono sample for both stereo output channels.
    pub fn next_sample(&mut self, settings: SynthSettings) -> f32 {
        if settings.attack_ms != self.cached_attack_ms {
            let samples = (settings.attack_ms as f64 * self.sample_rate * 0.001).max(1.0);
            self.attack_coefficient = (-4.6 / samples).exp() as f32;
            self.cached_attack_ms = settings.attack_ms;
        }
        if settings.release_ms != self.cached_release_ms {
            let samples = (settings.release_ms as f64 * self.sample_rate * 0.001).max(1.0);
            self.release_coefficient = (-9.2 / samples).exp() as f32;
            self.cached_release_ms = settings.release_ms;
        }
        let mut sum = 0.0;
        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            if voice.held {
                voice.envelope = 1.0 - (1.0 - voice.envelope) * self.attack_coefficient;
            } else {
                voice.envelope *= self.release_coefficient;
                if voice.envelope < 0.0001 {
                    voice.active = false;
                    continue;
                }
            }
            let step = (voice.frequency / self.sample_rate).clamp(0.0, 0.49);
            let oscillator = match settings.waveform {
                Waveform::Triangle => 1.0 - 4.0 * (voice.phase - 0.5).abs(),
                Waveform::Square => {
                    let raw = if voice.phase < 0.5 { 1.0 } else { -1.0 };
                    raw + poly_blep(voice.phase, step)
                        - poly_blep((voice.phase + 0.5).fract(), step)
                }
                Waveform::Saw => 2.0 * voice.phase - 1.0 - poly_blep(voice.phase, step),
            };
            sum += oscillator as f32 * voice.envelope * voice.velocity;
            voice.phase = (voice.phase + step).fract();
        }
        (sum * settings.level * 0.65).tanh()
    }

    /// Number of active voices, useful for targeted tests and status displays.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| voice.active).count()
    }

    /// MIDI keys with voices still held by the keyboard.
    pub fn held_key_mask(&self) -> [u64; 2] {
        let mut keys = [0_u64; 2];
        for voice in self
            .voices
            .iter()
            .filter(|voice| voice.active && voice.held)
        {
            let key = usize::from(voice.key);
            keys[key / 64] |= 1_u64 << (key % 64);
        }
        keys
    }
}

fn valid_rate(rate: f64) -> f64 {
    if rate.is_finite() && rate >= 8_000.0 {
        rate
    } else {
        48_000.0
    }
}

fn poly_blep(phase: f64, step: f64) -> f64 {
    if step <= 0.0 {
        return 0.0;
    }
    if phase < step {
        let t = phase / step;
        t + t - t * t - 1.0
    } else if phase > 1.0 - step {
        let t = (phase - 1.0) / step;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_attack_and_release_have_an_audible_tail_then_stop() {
        let mut synth = Synth::new(48_000.0);
        let settings = SynthSettings::default();
        synth.note_on(60, 0, 9, 0.8, 220.0);
        let peak = (0..24_000)
            .map(|_| synth.next_sample(settings).abs())
            .fold(0.0_f32, f32::max);
        assert!(peak > 0.01);
        synth.note_off(60, 0, 9);
        assert_eq!(synth.active_voices(), 1);
        let early_tail = (0..2_400)
            .map(|_| synth.next_sample(settings).abs())
            .fold(0.0_f32, f32::max);
        assert!(early_tail > 0.001);
        for _ in 0..60_000 {
            synth.next_sample(settings);
        }
        assert_eq!(synth.active_voices(), 0);
    }

    #[test]
    fn all_waveforms_render_finite_bounded_samples() {
        let mut synth = Synth::new(44_100.0);
        synth.note_on(99, 0, -1, 1.0, 10_000.0);
        for waveform in [Waveform::Triangle, Waveform::Square, Waveform::Saw] {
            let settings = SynthSettings {
                waveform,
                ..SynthSettings::default()
            };
            for _ in 0..1_000 {
                let sample = synth.next_sample(settings);
                assert!(sample.is_finite() && sample.abs() <= 1.0);
            }
        }
    }

    #[test]
    fn held_key_mask_tracks_polyphonic_note_releases() {
        let mut synth = Synth::default();
        synth.note_on(60, 0, 1, 1.0, 220.0);
        synth.note_on(84, 0, 2, 1.0, 440.0);
        assert_eq!(synth.held_key_mask(), [1_u64 << 60, 1_u64 << 20]);
        synth.note_off(60, 0, 1);
        assert_eq!(synth.held_key_mask(), [0, 1_u64 << 20]);
        synth.note_off(84, 0, 2);
        assert_eq!(synth.held_key_mask(), [0, 0]);
    }
}
