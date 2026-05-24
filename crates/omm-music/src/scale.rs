use serde::{Deserialize, Serialize};

use crate::pitch::{Pitch, PitchClass};

/// The seven church modes. `Major` and `Minor` are aliases (constants),
/// not separate variants, so callers can write `Mode::Major` while the
/// serialized form uses the underlying mode name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    Ionian,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Aeolian,
    Locrian,
}

impl Mode {
    /// Alias for `Mode::Ionian`.
    pub const MAJOR: Self = Self::Ionian;
    /// Alias for `Mode::Aeolian`.
    pub const MINOR: Self = Self::Aeolian;

    /// Semitone intervals from the root for each of the 7 degrees of
    /// the mode. Index 0 is always 0 (the root itself).
    pub const fn intervals(self) -> [u8; 7] {
        match self {
            Mode::Ionian => [0, 2, 4, 5, 7, 9, 11],
            Mode::Dorian => [0, 2, 3, 5, 7, 9, 10],
            Mode::Phrygian => [0, 1, 3, 5, 7, 8, 10],
            Mode::Lydian => [0, 2, 4, 6, 7, 9, 11],
            Mode::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
            Mode::Aeolian => [0, 2, 3, 5, 7, 8, 10],
            Mode::Locrian => [0, 1, 3, 5, 6, 8, 10],
        }
    }
}

/// A diatonic scale: a root pitch class plus a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scale {
    pub root: PitchClass,
    pub mode: Mode,
}

impl Scale {
    pub const fn new(root: PitchClass, mode: Mode) -> Self {
        Self { root, mode }
    }

    /// All 7 pitch classes of the scale, in scale-degree order.
    pub fn pitch_classes(self) -> [PitchClass; 7] {
        let intervals = self.mode.intervals();
        let mut out = [PitchClass::C; 7];
        let mut i = 0;
        while i < 7 {
            out[i] = self.root.shift(intervals[i] as i32);
            i += 1;
        }
        out
    }

    /// True if `pitch` belongs to this scale (octave-invariant check).
    pub fn contains(self, pitch: Pitch) -> bool {
        let pc = pitch.to_pitch_class();
        self.pitch_classes().contains(&pc)
    }

    /// Resolve a scale degree (`1..=7`) at `octave` to a concrete MIDI
    /// pitch. `octave` follows the MIDI convention where C4 = octave 4
    /// = MIDI 60. Returns `None` for invalid degrees or out-of-range
    /// pitches.
    pub fn degree_to_pitch(self, degree: u8, octave: i8) -> Option<Pitch> {
        if !(1..=7).contains(&degree) {
            return None;
        }
        let pc = self.pitch_classes()[(degree - 1) as usize];
        let midi = (octave as i32 + 1) * 12 + pc.0 as i32;
        Pitch::new(midi.try_into().ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcs(values: &[u8]) -> Vec<PitchClass> {
        values.iter().map(|&v| PitchClass(v)).collect()
    }

    #[test]
    fn ionian_intervals_are_w_w_h_w_w_w_h() {
        let intervals = Mode::Ionian.intervals();
        assert_eq!(intervals, [0, 2, 4, 5, 7, 9, 11]);
    }

    #[test]
    fn all_seven_modes_match_standard_interval_patterns() {
        let expected: [(Mode, [u8; 7]); 7] = [
            (Mode::Ionian, [0, 2, 4, 5, 7, 9, 11]),
            (Mode::Dorian, [0, 2, 3, 5, 7, 9, 10]),
            (Mode::Phrygian, [0, 1, 3, 5, 7, 8, 10]),
            (Mode::Lydian, [0, 2, 4, 6, 7, 9, 11]),
            (Mode::Mixolydian, [0, 2, 4, 5, 7, 9, 10]),
            (Mode::Aeolian, [0, 2, 3, 5, 7, 8, 10]),
            (Mode::Locrian, [0, 1, 3, 5, 6, 8, 10]),
        ];
        for (mode, ivs) in expected {
            assert_eq!(mode.intervals(), ivs, "mode {mode:?}");
        }
    }

    #[test]
    fn major_minor_aliases_resolve_to_ionian_aeolian() {
        assert_eq!(Mode::MAJOR, Mode::Ionian);
        assert_eq!(Mode::MINOR, Mode::Aeolian);
    }

    #[test]
    fn c_major_scale_has_no_accidentals() {
        let s = Scale::new(PitchClass::C, Mode::Ionian);
        let pcs_got: Vec<_> = s.pitch_classes().to_vec();
        let pcs_want = pcs(&[0, 2, 4, 5, 7, 9, 11]);
        assert_eq!(pcs_got, pcs_want);
    }

    #[test]
    fn c_dorian_has_eb_and_bb() {
        let s = Scale::new(PitchClass::C, Mode::Dorian);
        let got: Vec<_> = s.pitch_classes().to_vec();
        let want = pcs(&[0, 2, 3, 5, 7, 9, 10]);
        assert_eq!(got, want, "C Dorian = C D Eb F G A Bb");
    }

    #[test]
    fn all_84_scales_round_trip_root_at_degree_one() {
        // For every (root, mode) combination the first pitch class
        // must equal the scale's root.
        for root_value in 0..12 {
            let root = PitchClass(root_value);
            for mode in [
                Mode::Ionian,
                Mode::Dorian,
                Mode::Phrygian,
                Mode::Lydian,
                Mode::Mixolydian,
                Mode::Aeolian,
                Mode::Locrian,
            ] {
                let scale = Scale::new(root, mode);
                let pcs = scale.pitch_classes();
                assert_eq!(pcs[0], root, "{scale:?} degree 1 should be the root");
                // Each degree must be the root plus the mode's interval.
                let intervals = mode.intervals();
                for i in 0..7 {
                    assert_eq!(pcs[i], root.shift(intervals[i] as i32));
                }
            }
        }
    }

    #[test]
    fn scale_contains_diatonic_pitches() {
        let c_major = Scale::new(PitchClass::C, Mode::Ionian);
        // Diatonic: C D E F G A B
        for &midi in &[60, 62, 64, 65, 67, 69, 71] {
            assert!(c_major.contains(Pitch(midi)), "{midi} should be in C major");
        }
        // Chromatic: C# D# F# G# A#
        for &midi in &[61, 63, 66, 68, 70] {
            assert!(
                !c_major.contains(Pitch(midi)),
                "{midi} should NOT be in C major"
            );
        }
    }

    #[test]
    fn degree_to_pitch_resolves_octaves_correctly() {
        let c_major = Scale::new(PitchClass::C, Mode::Ionian);
        // Degree 1 at octave 4 = middle C = MIDI 60
        assert_eq!(c_major.degree_to_pitch(1, 4), Some(Pitch(60)));
        // Degree 5 at octave 4 = G4 = MIDI 67
        assert_eq!(c_major.degree_to_pitch(5, 4), Some(Pitch(67)));
        // Degree 8 / 0 invalid
        assert_eq!(c_major.degree_to_pitch(0, 4), None);
        assert_eq!(c_major.degree_to_pitch(8, 4), None);
    }

    #[test]
    fn degree_to_pitch_rejects_out_of_range_octaves() {
        let c_major = Scale::new(PitchClass::C, Mode::Ionian);
        // octave -2 → MIDI -12 → out of range
        assert_eq!(c_major.degree_to_pitch(1, -2), None);
        // octave 10 with low degree might exceed 127
        assert_eq!(c_major.degree_to_pitch(7, 10), None);
    }

    #[test]
    fn serde_round_trip_for_scale_and_mode() {
        let s = Scale::new(PitchClass::F_SHARP, Mode::Lydian);
        let json = serde_json::to_string(&s).unwrap();
        let back: Scale = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);

        let mode_json = serde_json::to_string(&Mode::Dorian).unwrap();
        assert_eq!(mode_json, "\"Dorian\"");
    }
}
