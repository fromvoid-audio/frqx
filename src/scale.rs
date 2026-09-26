//! Editable scale and the MIDI-frequency table sent to MTS-ESP.

/// Maximum number of notes in one repeating period.
pub const MAX_DEGREES: usize = 48;
/// Minimum separation between neighboring degrees while dragging.
pub const MIN_GAP_CENTS: f32 = 1.0;

/// One repeating tuning period with a selected root degree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// Number of active degrees per period.
    pub count: usize,
    /// Ascending cent positions within the period.
    pub cents: [f32; MAX_DEGREES],
    /// MIDI key that plays the root frequency.
    pub root_key: u8,
    /// Frequency of the root key.
    pub root_hz: f64,
    /// Degree assigned to the root key.
    pub root_degree: usize,
    /// Size of one repeating period in ordinary cents.
    pub period_cents: f32,
}

impl Default for Scale {
    fn default() -> Self {
        let mut scale = Self::equal(7);
        scale.set_major_tet();
        scale
    }
}

impl Scale {
    /// Construct an equal division of a 2:1 period.
    pub fn equal(count: usize) -> Self {
        let count = count.clamp(3, MAX_DEGREES);
        let mut cents = [0.0; MAX_DEGREES];
        for (index, cent) in cents.iter_mut().enumerate().take(count) {
            *cent = index as f32 * 1200.0 / count as f32;
        }
        Self {
            count,
            cents,
            root_key: 60,
            root_hz: 220.0,
            root_degree: 0,
            period_cents: 1200.0,
        }
    }

    /// Load the default seven-degree 12-TET major pattern.
    pub fn set_major_tet(&mut self) {
        self.count = 7;
        self.period_cents = 1200.0;
        self.cents[..7].copy_from_slice(&[0.0, 200.0, 400.0, 500.0, 700.0, 900.0, 1100.0]);
        self.root_degree = 0;
    }

    /// Load a seven-note five-limit example.
    pub fn set_just_major(&mut self) {
        self.count = 7;
        self.period_cents = 1200.0;
        self.root_degree = 0;
        let ratios: [f64; 7] = [
            1.0,
            9.0 / 8.0,
            5.0 / 4.0,
            4.0 / 3.0,
            3.0 / 2.0,
            5.0 / 3.0,
            15.0 / 8.0,
        ];
        for (index, ratio) in ratios.into_iter().enumerate() {
            self.cents[index] = (1200.0 * ratio.log2()) as f32;
        }
    }

    /// Reset the count and pitches to equal divisions.
    pub fn set_equal(&mut self, count: usize) {
        let root_key = self.root_key;
        let root_hz = self.root_hz;
        let root_degree = self.root_degree;
        let period_cents = self.period_cents;
        *self = Self::equal(count);
        self.root_key = root_key;
        self.root_hz = root_hz;
        self.root_degree = root_degree.min(self.count - 1);
        self.set_period(period_cents);
    }

    /// Change note count while retaining existing pitches and the selected root.
    pub fn set_count(&mut self, count: usize) {
        let target = count.clamp(3, MAX_DEGREES);
        while self.count < target {
            let mut widest = (0.0, self.count - 1);
            for index in 0..self.count {
                let next = if index + 1 == self.count {
                    self.period_cents
                } else {
                    self.cents[index + 1]
                };
                let gap = next - self.cents[index];
                if gap > widest.0 {
                    widest = (gap, index);
                }
            }
            let insert_at = widest.1 + 1;
            let next = if insert_at == self.count {
                self.period_cents
            } else {
                self.cents[insert_at]
            };
            let cents = (self.cents[widest.1] + next) * 0.5;
            self.cents.copy_within(insert_at..self.count, insert_at + 1);
            self.cents[insert_at] = cents;
            self.count += 1;
            if insert_at <= self.root_degree {
                self.root_degree += 1;
            }
        }
        while self.count > target {
            let remove_at = if self.root_degree == self.count - 1 {
                self.count - 2
            } else {
                self.count - 1
            };
            self.cents.copy_within(remove_at + 1..self.count, remove_at);
            self.count -= 1;
            self.cents[self.count] = 0.0;
            if remove_at < self.root_degree {
                self.root_degree -= 1;
            }
        }
    }

    /// Stretch or compress the existing pattern to a new period size.
    pub fn set_period(&mut self, period_cents: f32) {
        if !period_cents.is_finite() || !(600.0..=2400.0).contains(&period_cents) {
            return;
        }
        let factor = period_cents / self.period_cents;
        for cents in self.cents.iter_mut().take(self.count) {
            *cents *= factor;
        }
        self.period_cents = period_cents;
    }

    /// Move one degree without changing order or the period endpoints.
    pub fn move_degree(&mut self, index: usize, cents: f32) -> bool {
        if index >= self.count || !cents.is_finite() {
            return false;
        }
        let lower = if index == 0 {
            0.0
        } else {
            self.cents[index - 1] + MIN_GAP_CENTS
        };
        let upper = if index + 1 == self.count {
            self.period_cents
        } else {
            self.cents[index + 1] - MIN_GAP_CENTS
        };
        let next = cents.clamp(lower, upper.max(lower));
        if (next - self.cents[index]).abs() < 0.001 {
            return false;
        }
        self.cents[index] = next;
        true
    }

    /// Return the absolute frequency assigned to one MIDI key.
    pub fn frequency(&self, key: u8) -> f64 {
        let offset = i32::from(key) - i32::from(self.root_key);
        let position = self.root_degree as i32 + offset;
        let period = position.div_euclid(self.count as i32);
        let degree = position.rem_euclid(self.count as i32) as usize;
        let cents = f64::from(self.cents[degree] - self.cents[self.root_degree])
            + f64::from(period) * f64::from(self.period_cents);
        self.root_hz * 2.0_f64.powf(cents / 1200.0)
    }

    /// Build the full single-channel MTS-ESP tuning table.
    pub fn frequencies(&self) -> [f64; 128] {
        std::array::from_fn(|key| self.frequency(key as u8))
    }

    /// Check persisted state before accepting it.
    pub fn valid(&self) -> bool {
        if !(3..=MAX_DEGREES).contains(&self.count)
            || self.root_key > 127
            || !self.root_hz.is_finite()
            || !(10.0..=20_000.0).contains(&self.root_hz)
            || self.root_degree >= self.count
            || !self.period_cents.is_finite()
            || !(600.0..=2400.0).contains(&self.period_cents)
            || !self.cents[0].is_finite()
            || self.cents[0] < 0.0
        {
            return false;
        }
        (1..self.count).all(|index| {
            self.cents[index].is_finite()
                && self.cents[index] >= self.cents[index - 1] + MIN_GAP_CENTS
                && self.cents[index] <= self.period_cents
        })
    }
}

/// An octave-reduced point of the fifth/third ratio lattice.
#[derive(Clone, Copy, Debug)]
pub struct LatticePoint {
    /// Number of 3:2 generator moves.
    pub fifths: i32,
    /// Number of 5:4 generator moves.
    pub thirds: i32,
    /// Octave-reduced position in cents.
    pub cents: f32,
}

impl LatticePoint {
    /// Create a point from two integer generator coordinates.
    pub fn new(fifths: i32, thirds: i32) -> Self {
        let cents = (f64::from(fifths) * 1200.0 * (3.0_f64 / 2.0).log2()
            + f64::from(thirds) * 1200.0 * (5.0_f64 / 4.0).log2())
        .rem_euclid(1200.0) as f32;
        Self {
            fifths,
            thirds,
            cents,
        }
    }

    /// Return this landmark as an octave-reduced integer frequency ratio.
    pub fn ratio(self) -> (i64, i64) {
        let mut numerator = 1_i64;
        let mut denominator = 1_i64;
        if self.fifths >= 0 {
            numerator *= 3_i64.pow(self.fifths as u32);
            denominator *= 2_i64.pow(self.fifths as u32);
        } else {
            numerator *= 2_i64.pow((-self.fifths) as u32);
            denominator *= 3_i64.pow((-self.fifths) as u32);
        }
        if self.thirds >= 0 {
            numerator *= 5_i64.pow(self.thirds as u32);
            denominator *= 4_i64.pow(self.thirds as u32);
        } else {
            numerator *= 4_i64.pow((-self.thirds) as u32);
            denominator *= 5_i64.pow((-self.thirds) as u32);
        }
        while numerator < denominator {
            numerator *= 2;
        }
        while numerator >= denominator * 2 {
            denominator *= 2;
        }
        let divisor = gcd(numerator, denominator);
        (numerator / divisor, denominator / divisor)
    }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}

/// Find a nearby simple-ratio landmark in a bounded fifth/third grid.
pub fn nearest_lattice(cents: f32) -> (LatticePoint, f32) {
    let mut best = LatticePoint::new(0, 0);
    let mut error = circular_cents(cents - best.cents);
    for fifths in -2..=2 {
        for thirds in -1..=1 {
            let candidate = LatticePoint::new(fifths, thirds);
            let candidate_error = circular_cents(cents - candidate.cents);
            let complexity = fifths.abs() + thirds.abs();
            let best_complexity = best.fifths.abs() + best.thirds.abs();
            if candidate_error.abs() + 0.01 < error.abs()
                || ((candidate_error.abs() - error.abs()).abs() <= 0.01
                    && complexity < best_complexity)
            {
                best = candidate;
                error = candidate_error;
            }
        }
    }
    (best, error)
}

fn circular_cents(delta: f32) -> f32 {
    (delta + 600.0).rem_euclid(1200.0) - 600.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octave_mapping_and_negative_keys_are_stable() {
        let scale = Scale::equal(7);
        assert!((scale.frequency(67) / scale.frequency(60) - 2.0).abs() < 1e-12);
        assert!((scale.frequency(53) / scale.frequency(60) - 0.5).abs() < 1e-12);
        assert!(scale
            .frequencies()
            .iter()
            .all(|hz| hz.is_finite() && *hz > 0.0));
    }

    #[test]
    fn degree_edit_stays_ordered() {
        let mut scale = Scale::equal(7);
        assert!(scale.move_degree(2, 500.0));
        assert!(scale.valid());
        assert!(scale.move_degree(0, 20.0));
        assert!(scale.valid());
    }

    #[test]
    fn selected_root_and_period_anchor_the_frequency_table() {
        let mut scale = Scale {
            root_degree: 4,
            root_hz: 330.0,
            ..Scale::default()
        };
        scale.set_period(1901.955);
        assert!((scale.frequency(60) - 330.0).abs() < 1e-9);
        assert!((scale.frequency(67) / scale.frequency(60) - 3.0).abs() < 0.0001);
        assert!((scale.frequency(53) / scale.frequency(60) - 1.0 / 3.0).abs() < 0.0001);
        assert!(scale.valid());
    }

    #[test]
    fn chromatic_keys_advance_one_note_in_a_24_note_period() {
        let mut scale = Scale::equal(24);
        scale.root_key = 60;
        scale.root_hz = 220.0;
        for offset in 0..24 {
            let expected = 220.0 * 2.0_f64.powf(offset as f64 / 24.0);
            assert!((scale.frequency(60 + offset as u8) - expected).abs() < 0.001);
        }
        assert!((scale.frequency(84) - 440.0).abs() < 0.001);
        assert!((scale.frequency(59) - 220.0 * 2.0_f64.powf(-1.0 / 24.0)).abs() < 0.001);
    }

    #[test]
    fn forty_eight_notes_span_one_period_in_keyboard_order() {
        let scale = Scale::equal(48);
        assert!((scale.frequency(60) - 220.0).abs() < 0.001);
        assert!(scale.frequency(61) > scale.frequency(60));
        assert!((scale.frequency(108) - 440.0).abs() < 0.001);
        assert!(scale.valid());
    }

    #[test]
    fn changing_note_count_keeps_existing_pitches_and_root() {
        let mut scale = Scale {
            root_degree: 4,
            ..Scale::default()
        };
        let original_root = scale.cents[4];
        scale.set_count(24);
        assert_eq!(scale.count, 24);
        assert_eq!(scale.cents[scale.root_degree], original_root);
        assert!(scale.valid());
        scale.set_count(7);
        assert_eq!(scale.count, 7);
        assert_eq!(scale.cents[scale.root_degree], original_root);
        assert!(scale.valid());
    }

    #[test]
    fn lattice_finds_fifth() {
        let (point, error) = nearest_lattice(700.0);
        assert_eq!((point.fifths, point.thirds), (1, 0));
        assert_eq!(point.ratio(), (3, 2));
        assert!((error + 1.955).abs() < 0.01);
    }
}
