use serde::{Deserialize, Serialize};

use crate::pitch::{Pitch, PitchClass};

/// Common chord qualities used by the LLM composition layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChordQuality {
    Maj,
    Min,
    Dim,
    Aug,
    Sus2,
    Sus4,
    Dom7,
    Maj7,
    Min7,
    Min7b5,
    Dim7,
    MinMaj7,
}

impl ChordQuality {
    /// Semitones from the root for each chord tone, in stacked order.
    pub const fn intervals(self) -> &'static [u8] {
        match self {
            ChordQuality::Maj => &[0, 4, 7],
            ChordQuality::Min => &[0, 3, 7],
            ChordQuality::Dim => &[0, 3, 6],
            ChordQuality::Aug => &[0, 4, 8],
            ChordQuality::Sus2 => &[0, 2, 7],
            ChordQuality::Sus4 => &[0, 5, 7],
            ChordQuality::Dom7 => &[0, 4, 7, 10],
            ChordQuality::Maj7 => &[0, 4, 7, 11],
            ChordQuality::Min7 => &[0, 3, 7, 10],
            ChordQuality::Min7b5 => &[0, 3, 6, 10],
            ChordQuality::Dim7 => &[0, 3, 6, 9],
            ChordQuality::MinMaj7 => &[0, 3, 7, 11],
        }
    }

    /// All 12 standard chord qualities in fixed order. Useful for
    /// fixture / golden tests.
    pub const ALL: [Self; 12] = [
        Self::Maj,
        Self::Min,
        Self::Dim,
        Self::Aug,
        Self::Sus2,
        Self::Sus4,
        Self::Dom7,
        Self::Maj7,
        Self::Min7,
        Self::Min7b5,
        Self::Dim7,
        Self::MinMaj7,
    ];
}

/// Chord extension above the basic triad / seventh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Extension {
    Nine,
    FlatNine,
    SharpNine,
    Eleven,
    SharpEleven,
    FlatThirteen,
    Thirteen,
}

impl Extension {
    /// Semitones above the root for this extension (in the second
    /// octave, so 9 = 14 semitones).
    pub const fn semitones(self) -> i8 {
        match self {
            Extension::Nine => 14,
            Extension::FlatNine => 13,
            Extension::SharpNine => 15,
            Extension::Eleven => 17,
            Extension::SharpEleven => 18,
            Extension::FlatThirteen => 20,
            Extension::Thirteen => 21,
        }
    }
}

/// A chord — root pitch class, quality, and any extensions.
///
/// Duplicates in `extensions` are tolerated but deduplicated by
/// `basic_voicing` so the resulting voicing never has repeated
/// pitches.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chord {
    pub root: PitchClass,
    pub quality: ChordQuality,
    pub extensions: Vec<Extension>,
}

impl Chord {
    pub fn new(root: PitchClass, quality: ChordQuality) -> Self {
        Self {
            root,
            quality,
            extensions: Vec::new(),
        }
    }

    pub fn with_extension(mut self, extension: Extension) -> Self {
        self.extensions.push(extension);
        self
    }

    /// Build a root-position voicing for this chord at the given root
    /// octave. `root_octave` follows the MIDI convention where C4 = 4.
    /// Pitches that would fall outside MIDI 0..=127 are skipped.
    /// The resulting voicing is sorted ascending.
    pub fn basic_voicing(&self, root_octave: i8) -> Voicing {
        let mut pitches: Vec<Pitch> =
            Vec::with_capacity(self.quality.intervals().len() + self.extensions.len());
        let root_midi = (root_octave as i32 + 1) * 12 + self.root.0 as i32;
        for &interval in self.quality.intervals() {
            let m = root_midi + interval as i32;
            if let Ok(midi) = u8::try_from(m) {
                if let Some(p) = Pitch::new(midi) {
                    pitches.push(p);
                }
            }
        }
        for ext in &self.extensions {
            let m = root_midi + ext.semitones() as i32;
            if let Ok(midi) = u8::try_from(m) {
                if let Some(p) = Pitch::new(midi) {
                    pitches.push(p);
                }
            }
        }
        pitches.sort_by_key(|p| p.0);
        pitches.dedup();
        Voicing { pitches }
    }
}

/// A specific arrangement of pitches realizing a chord. Always sorted
/// ascending.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Voicing {
    pub pitches: Vec<Pitch>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voicing_midis(v: &Voicing) -> Vec<u8> {
        v.pitches.iter().map(|p| p.0).collect()
    }

    #[test]
    fn c_major_root_voicing_is_c_e_g_at_octave_4() {
        let chord = Chord::new(PitchClass::C, ChordQuality::Maj);
        let v = chord.basic_voicing(4);
        assert_eq!(voicing_midis(&v), vec![60, 64, 67]);
    }

    #[test]
    fn c_maj7_root_voicing_is_c_e_g_b() {
        let chord = Chord::new(PitchClass::C, ChordQuality::Maj7);
        let v = chord.basic_voicing(4);
        assert_eq!(voicing_midis(&v), vec![60, 64, 67, 71]);
    }

    #[test]
    fn c_min7_root_voicing_is_c_eb_g_bb() {
        let chord = Chord::new(PitchClass::C, ChordQuality::Min7);
        let v = chord.basic_voicing(4);
        assert_eq!(voicing_midis(&v), vec![60, 63, 67, 70]);
    }

    #[test]
    fn c_dom9_extension_adds_d_two_octaves_up() {
        let chord = Chord::new(PitchClass::C, ChordQuality::Dom7).with_extension(Extension::Nine);
        let v = chord.basic_voicing(4);
        assert_eq!(voicing_midis(&v), vec![60, 64, 67, 70, 74]);
    }

    #[test]
    fn voicings_for_all_12_keys_x_12_qualities_match_intervals() {
        // The "144 chord voicing fixture" — verifies every (root × quality)
        // matches its standard semitone pattern.
        for root_value in 0..12 {
            let root = PitchClass(root_value);
            for quality in ChordQuality::ALL {
                let chord = Chord::new(root, quality);
                let v = chord.basic_voicing(4);
                let expected: Vec<u8> = quality
                    .intervals()
                    .iter()
                    .filter_map(|&i| {
                        let m = (4 + 1) * 12 + root_value as i32 + i as i32;
                        u8::try_from(m).ok().filter(|&m| m <= 127)
                    })
                    .collect();
                assert_eq!(voicing_midis(&v), expected, "{chord:?}");
            }
        }
    }

    #[test]
    fn basic_voicing_skips_out_of_range_pitches() {
        // Root at octave 10 → MIDI 132 (out of range). The first chord
        // tone is the root itself; if root is out of range every tone
        // should be skipped.
        let chord = Chord::new(PitchClass::C, ChordQuality::Maj);
        let v = chord.basic_voicing(10);
        assert!(v.pitches.is_empty(), "all tones out of range");
        // Root at octave -2 → MIDI -12 (out of range, negative).
        let v = chord.basic_voicing(-2);
        assert!(v.pitches.is_empty(), "all tones out of range");
    }

    #[test]
    fn extension_semitones_match_standard() {
        assert_eq!(Extension::Nine.semitones(), 14);
        assert_eq!(Extension::FlatNine.semitones(), 13);
        assert_eq!(Extension::SharpNine.semitones(), 15);
        assert_eq!(Extension::Eleven.semitones(), 17);
        assert_eq!(Extension::SharpEleven.semitones(), 18);
        assert_eq!(Extension::FlatThirteen.semitones(), 20);
        assert_eq!(Extension::Thirteen.semitones(), 21);
    }

    #[test]
    fn voicing_is_always_sorted_ascending() {
        // Extensions come after triad tones in the interval list, but
        // numerically they belong above. basic_voicing must sort.
        let chord = Chord::new(PitchClass::C, ChordQuality::Maj7).with_extension(Extension::Nine);
        let v = chord.basic_voicing(4);
        let midis = voicing_midis(&v);
        for i in 1..midis.len() {
            assert!(midis[i - 1] <= midis[i], "voicing not sorted: {midis:?}");
        }
    }

    #[test]
    fn duplicate_extensions_are_deduplicated_in_voicing() {
        let chord = Chord::new(PitchClass::C, ChordQuality::Maj7)
            .with_extension(Extension::Nine)
            .with_extension(Extension::Nine);
        let v = chord.basic_voicing(4);
        // C E G B + D5 (twice → once after dedup)
        assert_eq!(voicing_midis(&v), vec![60, 64, 67, 71, 74]);
    }

    #[test]
    fn serde_round_trip_for_chord_and_voicing() {
        let chord =
            Chord::new(PitchClass::F_SHARP, ChordQuality::Min7).with_extension(Extension::Eleven);
        let json = serde_json::to_string(&chord).unwrap();
        let back: Chord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, chord);

        let v = chord.basic_voicing(4);
        let json = serde_json::to_string(&v).unwrap();
        let back: Voicing = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }
}
