use crate::chord::{Chord, Voicing};
use crate::pitch::Pitch;

/// Greedy voice-leading: for each voice in `prev`, pick the chord tone
/// of `target` that is closest in semitones. If the smallest available
/// movement exceeds `max_voice_movement_semitones`, that voice falls
/// back to the corresponding root-position tone of `target`.
///
/// The resulting voicing is sorted ascending and deduplicated (so two
/// `prev` voices that snap to the same target tone do not produce
/// duplicate pitches).
pub fn voice_leading(
    prev: &Voicing,
    target: &Chord,
    max_voice_movement_semitones: u8,
    target_root_octave: i8,
) -> Voicing {
    let fallback = target.basic_voicing(target_root_octave);
    let candidates: Vec<Pitch> = fallback.pitches.clone();
    if candidates.is_empty() {
        return Voicing {
            pitches: Vec::new(),
        };
    }
    let mut out: Vec<Pitch> = Vec::with_capacity(prev.pitches.len().max(candidates.len()));
    for (i, voice) in prev.pitches.iter().enumerate() {
        let nearest = candidates
            .iter()
            .min_by_key(|c| (c.midi() as i32 - voice.midi() as i32).unsigned_abs())
            .copied()
            .unwrap_or(candidates[0]);
        let movement = (nearest.midi() as i32 - voice.midi() as i32).unsigned_abs() as u8;
        if movement <= max_voice_movement_semitones {
            out.push(nearest);
        } else {
            // Fall back to the i-th tone of the root-position voicing
            // (so all chord tones still get represented).
            out.push(candidates[i.min(candidates.len() - 1)]);
        }
    }
    // Ensure every chord tone is present at least once. This lets the
    // algorithm degrade gracefully when prev has fewer voices than
    // target has chord tones.
    for c in &candidates {
        if !out.contains(c) {
            out.push(*c);
        }
    }
    out.sort_by_key(|p| p.midi());
    out.dedup();
    Voicing { pitches: out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChordQuality, PitchClass};

    #[test]
    fn c_major_to_f_major_keeps_all_target_chord_tones() {
        let prev = Chord::new(PitchClass::C, ChordQuality::Maj).basic_voicing(4);
        let target = Chord::new(PitchClass::F, ChordQuality::Maj);
        let v = voice_leading(&prev, &target, 5, 4);
        let fmaj = target.basic_voicing(4);
        // All target chord tones must be present in the voiced result.
        for p in &fmaj.pitches {
            assert!(v.pitches.contains(p), "missing chord tone {p:?}");
        }
        // The voicing must contain at least the chord triad.
        assert!(v.pitches.len() >= fmaj.pitches.len());
    }

    #[test]
    fn nearest_chord_tone_is_within_max_movement_when_in_reach() {
        let prev = Chord::new(PitchClass::C, ChordQuality::Maj).basic_voicing(4);
        let target = Chord::new(PitchClass::F, ChordQuality::Maj); // F=65, A=69, C5=72
        let v = voice_leading(&prev, &target, 5, 4);
        // C4 (60) → 65 is 5 semitones — within max=5. Voicing should
        // therefore include 65 (not the fallback root-position tone).
        assert!(v.pitches.iter().any(|p| p.midi() == 65));
    }

    #[test]
    fn empty_prev_yields_full_target_voicing() {
        let prev = Voicing {
            pitches: Vec::new(),
        };
        let target = Chord::new(PitchClass::G, ChordQuality::Min7);
        let v = voice_leading(&prev, &target, 5, 4);
        // Should at least contain the target chord tones.
        let target_v = target.basic_voicing(4);
        for p in &target_v.pitches {
            assert!(v.pitches.contains(p));
        }
    }
}
