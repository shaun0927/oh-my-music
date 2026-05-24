use crate::chord::Chord;
use crate::pitch::{Pitch, PitchClass};
use crate::scale::Scale;

/// Snap every pitch in `pitches` to the nearest pitch-class member of
/// `scale`, preserving octave. Ties (equidistant up and down) prefer
/// the upward direction.
pub fn quantize_to_scale(pitches: &mut [Pitch], scale: Scale) {
    let valid = scale.pitch_classes();
    for p in pitches.iter_mut() {
        if let Some(snapped) = nearest_pitch_in_set(*p, &valid) {
            *p = snapped;
        }
    }
}

/// Snap every pitch in `pitches` to the nearest chord tone of `chord`
/// (root-position pitch-class set), preserving octave.
pub fn quantize_to_chord(pitches: &mut [Pitch], chord: &Chord) {
    let voicing = chord.basic_voicing(4);
    let valid: Vec<PitchClass> = voicing.pitches.iter().map(|p| p.to_pitch_class()).collect();
    if valid.is_empty() {
        return;
    }
    for p in pitches.iter_mut() {
        if let Some(snapped) = nearest_pitch_in_set(*p, &valid) {
            *p = snapped;
        }
    }
}

fn nearest_pitch_in_set(p: Pitch, valid: &[PitchClass]) -> Option<Pitch> {
    if valid.is_empty() {
        return None;
    }
    let target_pc = p.to_pitch_class();
    if valid.contains(&target_pc) {
        return Some(p);
    }
    // Search outward from the current pitch.
    let p_midi = p.midi() as i32;
    for delta in 1..=6 {
        for sign in [1, -1] {
            let candidate = p_midi + sign * delta;
            if (0..=127).contains(&candidate) {
                let pc = PitchClass((candidate as u8) % 12);
                if valid.contains(&pc) {
                    return Some(Pitch(candidate as u8));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChordQuality, Mode};

    #[test]
    fn quantize_to_c_major_snaps_accidentals() {
        let scale = Scale::new(PitchClass::C, Mode::Ionian);
        let mut pitches = vec![
            Pitch(60),
            Pitch(61),
            Pitch(63),
            Pitch(66),
            Pitch(68),
            Pitch(70),
        ];
        quantize_to_scale(&mut pitches, scale);
        // All snapped pitches must belong to C major.
        for p in &pitches {
            assert!(scale.contains(*p), "{p:?} not in scale after quantize");
        }
    }

    #[test]
    fn quantize_to_chord_snaps_to_chord_tones() {
        let chord = Chord::new(PitchClass::C, ChordQuality::Maj); // C E G
        let mut pitches = vec![
            Pitch(60),
            Pitch(61),
            Pitch(62),
            Pitch(63),
            Pitch(64),
            Pitch(65),
        ];
        quantize_to_chord(&mut pitches, &chord);
        // Every pitch's class must be C, E, or G (0/4/7).
        for p in &pitches {
            let pc = p.to_pitch_class().value();
            assert!(matches!(pc, 0 | 4 | 7), "{p:?} pc={pc} not C/E/G");
        }
    }

    #[test]
    fn quantize_thousand_random_pitches_all_land_in_scale() {
        let scale = Scale::new(PitchClass::G, Mode::Dorian);
        let mut pitches: Vec<Pitch> = (0..1000_u32)
            .map(|i| Pitch(((i * 7) % 128) as u8))
            .collect();
        quantize_to_scale(&mut pitches, scale);
        for p in &pitches {
            assert!(scale.contains(*p));
        }
    }
}
