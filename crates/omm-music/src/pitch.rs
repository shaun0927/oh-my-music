use serde::{Deserialize, Serialize};

/// A MIDI pitch in `0..=127`. `Pitch(60)` is middle C, `Pitch(69)` is
/// A4 = 440 Hz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pitch(pub u8);

impl Pitch {
    /// Construct a pitch from MIDI value. Returns `None` if `midi > 127`.
    pub const fn new(midi: u8) -> Option<Self> {
        if midi <= 127 {
            Some(Self(midi))
        } else {
            None
        }
    }

    /// Saturating constructor — clamps `midi` into `0..=127`.
    pub const fn clamped(midi: i32) -> Self {
        if midi < 0 {
            Self(0)
        } else if midi > 127 {
            Self(127)
        } else {
            Self(midi as u8)
        }
    }

    /// Raw MIDI value.
    pub const fn midi(self) -> u8 {
        self.0
    }

    /// Pitch class (octave dropped, 0..=11).
    pub const fn to_pitch_class(self) -> PitchClass {
        PitchClass(self.0 % 12)
    }

    /// Transpose by `interval` semitones. Returns `None` if the result
    /// falls outside `0..=127`.
    pub fn transpose(self, interval: Interval) -> Option<Self> {
        let new = self.0 as i32 + interval.0 as i32;
        Self::new(new.try_into().ok()?)
    }

    /// Saturating transpose — clamps result into `0..=127`.
    pub fn transpose_clamped(self, interval: Interval) -> Self {
        Self::clamped(self.0 as i32 + interval.0 as i32)
    }

    /// Convert to frequency in Hz using standard equal temperament
    /// (A4 = 440 Hz, MIDI 69 = 440 Hz).
    pub fn to_freq_hz(self) -> f32 {
        440.0_f32 * 2.0_f32.powf((self.0 as f32 - 69.0) / 12.0)
    }
}

/// A pitch class (octave-invariant), `0..=11`. `0 = C, 1 = C#/Db, …, 11 = B`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PitchClass(pub u8);

impl PitchClass {
    pub const C: Self = Self(0);
    pub const C_SHARP: Self = Self(1);
    pub const D: Self = Self(2);
    pub const D_SHARP: Self = Self(3);
    pub const E: Self = Self(4);
    pub const F: Self = Self(5);
    pub const F_SHARP: Self = Self(6);
    pub const G: Self = Self(7);
    pub const G_SHARP: Self = Self(8);
    pub const A: Self = Self(9);
    pub const A_SHARP: Self = Self(10);
    pub const B: Self = Self(11);

    pub const fn new(pc: u8) -> Option<Self> {
        if pc < 12 {
            Some(Self(pc))
        } else {
            None
        }
    }

    /// Add `semitones` (positive or negative), wrapping into `0..=11`.
    /// Named `shift` (not `add`) to avoid colliding with `std::ops::Add`.
    pub fn shift(self, semitones: i32) -> Self {
        Self((self.0 as i32 + semitones).rem_euclid(12) as u8)
    }

    /// Raw pitch class value in `0..=11`.
    pub const fn value(self) -> u8 {
        self.0
    }
}

/// A musical interval in semitones. Positive = up, negative = down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Interval(pub i8);

impl Interval {
    pub const UNISON: Self = Self(0);
    pub const MINOR_SECOND: Self = Self(1);
    pub const MAJOR_SECOND: Self = Self(2);
    pub const MINOR_THIRD: Self = Self(3);
    pub const MAJOR_THIRD: Self = Self(4);
    pub const PERFECT_FOURTH: Self = Self(5);
    pub const TRITONE: Self = Self(6);
    pub const PERFECT_FIFTH: Self = Self(7);
    pub const MINOR_SIXTH: Self = Self(8);
    pub const MAJOR_SIXTH: Self = Self(9);
    pub const MINOR_SEVENTH: Self = Self(10);
    pub const MAJOR_SEVENTH: Self = Self(11);
    pub const OCTAVE: Self = Self(12);

    pub const fn semitones(self) -> i8 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_constructors_validate_range() {
        assert_eq!(Pitch::new(0), Some(Pitch(0)));
        assert_eq!(Pitch::new(60), Some(Pitch(60)));
        assert_eq!(Pitch::new(127), Some(Pitch(127)));
        assert_eq!(Pitch::new(128), None);
        assert_eq!(Pitch::clamped(-10), Pitch(0));
        assert_eq!(Pitch::clamped(200), Pitch(127));
    }

    #[test]
    fn pitch_to_pitch_class_drops_octave() {
        assert_eq!(Pitch(60).to_pitch_class(), PitchClass::C);
        assert_eq!(Pitch(72).to_pitch_class(), PitchClass::C);
        assert_eq!(Pitch(69).to_pitch_class(), PitchClass::A);
    }

    #[test]
    fn pitch_transpose_property_octave_preserves_pitch_class() {
        for midi in 0_u8..=127 {
            let p = Pitch(midi);
            let pc = p.to_pitch_class();
            // up an octave (when in range)
            if let Some(up) = p.transpose(Interval::OCTAVE) {
                assert_eq!(up.to_pitch_class(), pc, "+12 should preserve pitch class");
            }
            // down an octave (when in range)
            if let Some(down) = p.transpose(Interval(-12)) {
                assert_eq!(down.to_pitch_class(), pc, "-12 should preserve pitch class");
            }
        }
    }

    #[test]
    fn pitch_transpose_returns_none_outside_range() {
        assert_eq!(Pitch(0).transpose(Interval(-1)), None);
        assert_eq!(Pitch(127).transpose(Interval(1)), None);
        assert_eq!(
            Pitch(0).transpose_clamped(Interval(-50)),
            Pitch(0),
            "clamped"
        );
        assert_eq!(
            Pitch(127).transpose_clamped(Interval(50)),
            Pitch(127),
            "clamped"
        );
    }

    #[test]
    fn pitch_to_freq_hz_matches_equal_temperament() {
        assert!((Pitch(69).to_freq_hz() - 440.0).abs() < 0.001, "A4");
        assert!((Pitch(60).to_freq_hz() - 261.6256).abs() < 0.001, "C4");
        assert!((Pitch(72).to_freq_hz() - 523.2511).abs() < 0.001, "C5");
        assert!(
            Pitch(0).to_freq_hz() > 8.0 && Pitch(0).to_freq_hz() < 9.0,
            "MIDI 0 ≈ 8.176 Hz"
        );
    }

    #[test]
    fn pitch_class_add_wraps_modulo_twelve() {
        assert_eq!(PitchClass::C.shift(12), PitchClass::C);
        assert_eq!(PitchClass::C.shift(-1), PitchClass::B);
        assert_eq!(PitchClass::B.shift(1), PitchClass::C);
        assert_eq!(PitchClass::C.shift(7), PitchClass::G);
        assert_eq!(PitchClass::A.shift(-9), PitchClass::C);
    }

    #[test]
    fn serde_round_trip() {
        for p in [Pitch(0), Pitch(60), Pitch(127)] {
            let s = serde_json::to_string(&p).unwrap();
            let back: Pitch = serde_json::from_str(&s).unwrap();
            assert_eq!(p, back);
        }
        let pc = PitchClass::F_SHARP;
        let s = serde_json::to_string(&pc).unwrap();
        assert_eq!(s, "6");
        let interval = Interval::PERFECT_FIFTH;
        let s = serde_json::to_string(&interval).unwrap();
        assert_eq!(s, "7");
    }
}
