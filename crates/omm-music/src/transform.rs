use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::note::Note;
use crate::pitch::{Interval, Pitch};

/// Transpose every note by `by` semitones in place. Notes that would
/// fall outside MIDI 0..=127 are clamped saturatingly.
pub fn transpose(notes: &mut [Note], by: Interval) {
    for n in notes.iter_mut() {
        n.pitch = n.pitch.transpose_clamped(by);
    }
}

/// Invert every note around `axis`: `pitch -> 2 * axis - pitch`,
/// clamped to MIDI range.
pub fn invert(notes: &mut [Note], axis: Pitch) {
    for n in notes.iter_mut() {
        let new = 2 * axis.midi() as i32 - n.pitch.midi() as i32;
        n.pitch = Pitch::clamped(new);
    }
}

/// Reverse note order in time. Each note's `start_ticks` is mirrored
/// around the total span so the last note becomes first and so on.
/// Note `length_ticks` is preserved.
pub fn retrograde(notes: &mut [Note]) {
    if notes.is_empty() {
        return;
    }
    let span = notes
        .iter()
        .map(|n| n.start_ticks + n.length_ticks)
        .max()
        .unwrap_or(0);
    for n in notes.iter_mut() {
        let end = n.start_ticks + n.length_ticks;
        n.start_ticks = span.saturating_sub(end);
    }
    notes.sort_by_key(|n| n.start_ticks);
}

/// Scale every note's `start_ticks` and `length_ticks` by `factor`
/// (rounded). `factor < 1.0` compresses, `> 1.0` stretches. Negative
/// or non-finite factors are ignored.
pub fn augment(notes: &mut [Note], factor: f32) {
    if !factor.is_finite() || factor <= 0.0 {
        return;
    }
    for n in notes.iter_mut() {
        n.start_ticks = ((n.start_ticks as f32 * factor).round() as i64).max(0) as u32;
        n.length_ticks = ((n.length_ticks as f32 * factor).round() as i64).max(1) as u32;
    }
}

/// Add bounded random jitter to velocity and start position. `seed`
/// makes the transformation deterministic. `velocity_jitter` is the
/// maximum absolute change applied to each velocity (clamped to
/// 0..=127). `timing_jitter_ticks` is applied symmetrically to each
/// `start_ticks` and clamped at zero (cannot go negative).
pub fn humanize(notes: &mut [Note], velocity_jitter: u8, timing_jitter_ticks: u32, seed: u64) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    for n in notes.iter_mut() {
        if velocity_jitter > 0 {
            let delta: i32 = rng.gen_range(-(velocity_jitter as i32)..=velocity_jitter as i32);
            let v = (n.velocity as i32 + delta).clamp(0, 127);
            n.velocity = v as u8;
        }
        if timing_jitter_ticks > 0 {
            let delta: i32 =
                rng.gen_range(-(timing_jitter_ticks as i32)..=timing_jitter_ticks as i32);
            n.start_ticks = n.start_ticks.saturating_add_signed(delta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nt(pitch: u8, start: u32, length: u32) -> Note {
        Note::new(Pitch(pitch), 100, start, length)
    }

    #[test]
    fn transpose_by_octave_then_back_is_identity_on_pitch() {
        let original = vec![nt(60, 0, 480), nt(64, 480, 480), nt(67, 960, 480)];
        let mut notes = original.clone();
        transpose(&mut notes, Interval::OCTAVE);
        transpose(&mut notes, Interval(-12));
        assert_eq!(notes, original);
    }

    #[test]
    fn transpose_clamps_out_of_range() {
        let mut notes = vec![nt(125, 0, 480)];
        transpose(&mut notes, Interval(10));
        assert_eq!(notes[0].pitch, Pitch(127));
    }

    #[test]
    fn invert_around_center_preserves_distance() {
        let mut notes = vec![nt(60, 0, 480)];
        invert(&mut notes, Pitch(64));
        // 60 inverted around 64 → 68.
        assert_eq!(notes[0].pitch, Pitch(68));
    }

    #[test]
    fn retrograde_twice_is_identity() {
        let original = vec![nt(60, 0, 240), nt(62, 240, 240), nt(64, 480, 240)];
        let mut notes = original.clone();
        retrograde(&mut notes);
        retrograde(&mut notes);
        assert_eq!(notes, original);
    }

    #[test]
    fn augment_double_doubles_lengths() {
        let mut notes = vec![nt(60, 0, 240)];
        augment(&mut notes, 2.0);
        assert_eq!(notes[0].length_ticks, 480);
    }

    #[test]
    fn humanize_is_deterministic_with_seed() {
        let original = vec![nt(60, 0, 240); 50];
        let mut a = original.clone();
        let mut b = original.clone();
        humanize(&mut a, 10, 30, 7);
        humanize(&mut b, 10, 30, 7);
        assert_eq!(a, b);
    }

    #[test]
    fn humanize_velocity_stays_in_range() {
        let mut notes = vec![nt(60, 0, 240); 200];
        humanize(&mut notes, 50, 0, 1);
        for n in &notes {
            assert!(n.velocity <= 127);
        }
    }
}
