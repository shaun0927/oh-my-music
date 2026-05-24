use crate::chord::{Chord, Voicing};
use crate::pitch::Pitch;

/// Result of `voice_leading_optimal`: the new voicing plus the
/// per-voice assignment (`assignment[i]` is the target pitch that
/// `prev.pitches[i]` was mapped to).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceLeadingResult {
    pub voicing: Voicing,
    pub assignment: Vec<Pitch>,
    pub total_movement_semitones: u32,
}

/// Optimal voice-leading via brute-force enumeration of the
/// assignment matrix (Hungarian-equivalent for the small n ≤ 8 case
/// the LLM picks). For each prev voice, picks a UNIQUE target chord
/// tone such that the sum of absolute pitch movements is minimized.
///
/// When `prev` has fewer voices than the target has chord tones, the
/// unassigned target tones are still included in the final voicing
/// so every chord tone sounds. When `prev` has more voices than the
/// target has tones, the optimal n×k assignment is taken where
/// k = target tones and the extra prev voices fall back to their
/// nearest non-collision target (typically a duplicate of one
/// already taken — folded out by the final dedup).
pub fn voice_leading_optimal(
    prev: &Voicing,
    target: &Chord,
    target_root_octave: i8,
) -> VoiceLeadingResult {
    let targets = target.basic_voicing(target_root_octave).pitches;
    if prev.pitches.is_empty() || targets.is_empty() {
        return VoiceLeadingResult {
            voicing: Voicing {
                pitches: targets.clone(),
            },
            assignment: Vec::new(),
            total_movement_semitones: 0,
        };
    }
    let n = prev.pitches.len();
    let m = targets.len();
    let k = n.min(m);

    let mut best_cost: u32 = u32::MAX;
    let mut best_assignment: Vec<Pitch> = Vec::new();
    let candidate_indices: Vec<usize> = (0..m).collect();

    enumerate_k_permutations(&candidate_indices, k, &mut |perm| {
        let mut cost: u32 = 0;
        for (i, &j) in perm.iter().enumerate() {
            cost = cost.saturating_add(
                (prev.pitches[i].midi() as i32 - targets[j].midi() as i32).unsigned_abs(),
            );
        }
        if cost < best_cost {
            best_cost = cost;
            best_assignment = perm.iter().map(|&j| targets[j]).collect();
        }
    });

    // For prev voices beyond `k`, fall back to their nearest target.
    for i in k..n {
        let nearest = targets
            .iter()
            .min_by_key(|t| (t.midi() as i32 - prev.pitches[i].midi() as i32).unsigned_abs())
            .copied()
            .unwrap_or(targets[0]);
        best_assignment.push(nearest);
        best_cost = best_cost.saturating_add(
            (nearest.midi() as i32 - prev.pitches[i].midi() as i32).unsigned_abs(),
        );
    }

    // Build the final pitch set: assigned tones + any missing chord
    // tones, deduped and sorted.
    let mut pitches = best_assignment.clone();
    for t in &targets {
        if !pitches.contains(t) {
            pitches.push(*t);
        }
    }
    pitches.sort_by_key(|p| p.midi());
    pitches.dedup();

    VoiceLeadingResult {
        voicing: Voicing { pitches },
        assignment: best_assignment,
        total_movement_semitones: best_cost,
    }
}

fn enumerate_k_permutations(set: &[usize], k: usize, f: &mut impl FnMut(&[usize])) {
    if k == 0 {
        f(&[]);
        return;
    }
    let mut current = vec![0_usize; k];
    let mut used = vec![false; set.len()];
    perm_recurse(set, &mut current, 0, &mut used, f);
}

fn perm_recurse(
    set: &[usize],
    current: &mut Vec<usize>,
    depth: usize,
    used: &mut Vec<bool>,
    f: &mut impl FnMut(&[usize]),
) {
    if depth == current.len() {
        f(current);
        return;
    }
    for i in 0..set.len() {
        if !used[i] {
            used[i] = true;
            current[depth] = set[i];
            perm_recurse(set, current, depth + 1, used, f);
            used[i] = false;
        }
    }
}

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

    // -- A3: voice_leading_optimal --

    #[test]
    fn optimal_voice_leading_minimizes_total_movement_on_triad_to_triad() {
        // C major (60, 64, 67) → F major (65, 69, 72).
        // Greedy collapses everyone onto 65; optimal must give the
        // unique-assignment minimum: 60→65 (5) + 64→69 (5) + 67→72 (5)
        // = 15 OR another permutation with same total. Any valid
        // optimal solution sums to 15 — greedy with collapse would
        // report something higher (or the wrong shape).
        let prev = Chord::new(PitchClass::C, ChordQuality::Maj).basic_voicing(4);
        let target = Chord::new(PitchClass::F, ChordQuality::Maj);
        let result = voice_leading_optimal(&prev, &target, 4);
        assert_eq!(result.total_movement_semitones, 15);
        // Assignment must be a true bijection: 3 distinct target tones.
        assert_eq!(result.assignment.len(), 3);
        let mut unique = result.assignment.clone();
        unique.sort_by_key(|p| p.midi());
        unique.dedup();
        assert_eq!(unique.len(), 3, "optimal must not double-assign");
    }

    #[test]
    fn optimal_voice_leading_handles_close_chord_change_with_zero_movement() {
        // C major (60, 64, 67) → A minor (57, 60, 64).
        // 60 → 60 (0), 64 → 64 (0), 67 → 57 (-10). Total 10.
        // No better arrangement exists.
        let prev = Chord::new(PitchClass::C, ChordQuality::Maj).basic_voicing(4);
        let target = Chord::new(PitchClass::A, ChordQuality::Min);
        let result = voice_leading_optimal(&prev, &target, 3);
        assert!(
            result.total_movement_semitones <= 10,
            "expected ≤ 10, got {}",
            result.total_movement_semitones
        );
    }

    #[test]
    fn optimal_voice_leading_with_empty_prev_returns_target_tones() {
        let prev = Voicing {
            pitches: Vec::new(),
        };
        let target = Chord::new(PitchClass::G, ChordQuality::Maj7);
        let result = voice_leading_optimal(&prev, &target, 4);
        let basic = target.basic_voicing(4);
        for p in &basic.pitches {
            assert!(result.voicing.pitches.contains(p));
        }
        assert_eq!(result.total_movement_semitones, 0);
    }

    #[test]
    fn optimal_voice_leading_assignment_is_unique_per_voice() {
        // 4 prev voices, 4 chord tones — full bijection.
        let prev = Chord::new(PitchClass::C, ChordQuality::Maj7).basic_voicing(4);
        let target = Chord::new(PitchClass::G, ChordQuality::Maj7);
        let result = voice_leading_optimal(&prev, &target, 4);
        assert_eq!(result.assignment.len(), 4);
        let mut unique = result.assignment.clone();
        unique.sort_by_key(|p| p.midi());
        unique.dedup();
        assert_eq!(unique.len(), 4);
    }
}
