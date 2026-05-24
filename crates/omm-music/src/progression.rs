use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::chord::{Chord, ChordQuality};
use crate::pitch::PitchClass;
use crate::scale::{Mode, Scale};

/// Canonical progression styles the LLM can request as a one-word
/// option without having to enumerate chord-by-chord intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProgressionStyle {
    /// I – V – vi – IV (pop). Major mode only; falls back to seeded
    /// random selection if the key is non-major.
    PopIVviIV,
    /// ii – V – I – vi (jazz). Major mode only; falls back to seeded
    /// random selection if the key is non-major.
    JazzIIVI,
    /// i – IV – i – ♭VII (modal Dorian flavor).
    ModalDorian,
    /// 12-bar blues. Only meaningful in 4-bar increments — the function
    /// returns the first `length_bars` chords of the standard 12-bar
    /// progression.
    BluesTwelveBar,
    /// Seed-driven random walk through diatonic chords of the scale.
    Random,
}

/// Generate a chord progression of `length_bars` bars (one chord per
/// bar). All four canonical styles are deterministic given `seed`.
/// Mode mismatches degrade to a seeded random walk rather than panic.
pub fn generate_progression(
    key: Scale,
    length_bars: u8,
    style: ProgressionStyle,
    seed: u64,
) -> Vec<Chord> {
    let length = length_bars as usize;
    if length == 0 {
        return Vec::new();
    }
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    match style {
        ProgressionStyle::PopIVviIV if key.mode == Mode::Ionian => {
            // I V vi IV in major.
            let cycle = [
                diatonic_chord(key, 1),
                diatonic_chord(key, 5),
                diatonic_chord(key, 6),
                diatonic_chord(key, 4),
            ];
            cycle.iter().cycle().take(length).cloned().collect()
        }
        ProgressionStyle::JazzIIVI if key.mode == Mode::Ionian => {
            let cycle = [
                diatonic_chord(key, 2),
                diatonic_chord(key, 5),
                diatonic_chord(key, 1),
                diatonic_chord(key, 6),
            ];
            cycle.iter().cycle().take(length).cloned().collect()
        }
        ProgressionStyle::ModalDorian => {
            let dorian = Scale::new(key.root, Mode::Dorian);
            let cycle = [
                diatonic_chord(dorian, 1),
                diatonic_chord(dorian, 4),
                diatonic_chord(dorian, 1),
                Chord::new(dorian.root.shift(10), ChordQuality::Maj),
            ];
            cycle.iter().cycle().take(length).cloned().collect()
        }
        ProgressionStyle::BluesTwelveBar => {
            // Standard 12-bar blues with dominant 7ths.
            let i = Chord::new(key.root, ChordQuality::Dom7);
            let iv = Chord::new(key.root.shift(5), ChordQuality::Dom7);
            let v = Chord::new(key.root.shift(7), ChordQuality::Dom7);
            let cycle = [
                i.clone(),
                i.clone(),
                i.clone(),
                i.clone(),
                iv.clone(),
                iv.clone(),
                i.clone(),
                i.clone(),
                v.clone(),
                iv.clone(),
                i.clone(),
                v,
            ];
            cycle.iter().cycle().take(length).cloned().collect()
        }
        // Fallback / Random: pick diatonic chord per bar from the
        // active mode's seven scale degrees.
        _ => (0..length)
            .map(|_| {
                let degree: u8 = rng.gen_range(1..=7);
                diatonic_chord(key, degree)
            })
            .collect(),
    }
}

/// Build the diatonic triad at `degree` (1..=7) of `scale`.
fn diatonic_chord(scale: Scale, degree: u8) -> Chord {
    let degree = degree.clamp(1, 7);
    let pcs = scale.pitch_classes();
    let root = pcs[(degree - 1) as usize];
    // Third = degree+2 mod 7, fifth = degree+4 mod 7. Compute the
    // interval distance from root to choose quality.
    let third_pc = pcs[(degree as usize + 1) % 7];
    let fifth_pc = pcs[(degree as usize + 3) % 7];
    let third_interval = pitch_class_distance(root, third_pc);
    let fifth_interval = pitch_class_distance(root, fifth_pc);
    let quality = match (third_interval, fifth_interval) {
        (4, 7) => ChordQuality::Maj,
        (3, 7) => ChordQuality::Min,
        (3, 6) => ChordQuality::Dim,
        (4, 8) => ChordQuality::Aug,
        // Fallback: treat anything else as Major.
        _ => ChordQuality::Maj,
    };
    Chord::new(root, quality)
}

fn pitch_class_distance(from: PitchClass, to: PitchClass) -> i32 {
    (to.value() as i32 - from.value() as i32).rem_euclid(12)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pop_progression_c_major_is_c_g_am_f() {
        let key = Scale::new(PitchClass::C, Mode::Ionian);
        let p = generate_progression(key, 4, ProgressionStyle::PopIVviIV, 0);
        let roots: Vec<u8> = p.iter().map(|c| c.root.value()).collect();
        // C(0) G(7) A(9) F(5)
        assert_eq!(roots, vec![0, 7, 9, 5]);
        // Qualities: Maj Maj Min Maj for I V vi IV in major.
        assert_eq!(p[0].quality, ChordQuality::Maj);
        assert_eq!(p[1].quality, ChordQuality::Maj);
        assert_eq!(p[2].quality, ChordQuality::Min);
        assert_eq!(p[3].quality, ChordQuality::Maj);
    }

    #[test]
    fn jazz_progression_c_major_is_dm_g_c_am() {
        let key = Scale::new(PitchClass::C, Mode::Ionian);
        let p = generate_progression(key, 4, ProgressionStyle::JazzIIVI, 0);
        let roots: Vec<u8> = p.iter().map(|c| c.root.value()).collect();
        // D(2) G(7) C(0) A(9)
        assert_eq!(roots, vec![2, 7, 0, 9]);
    }

    #[test]
    fn blues_progression_c_first_four_bars_are_i_i_i_i() {
        let key = Scale::new(PitchClass::C, Mode::Ionian);
        let p = generate_progression(key, 4, ProgressionStyle::BluesTwelveBar, 0);
        for ch in &p {
            assert_eq!(ch.root, PitchClass::C);
            assert_eq!(ch.quality, ChordQuality::Dom7);
        }
    }

    #[test]
    fn blues_progression_c_bar5_is_iv() {
        let key = Scale::new(PitchClass::C, Mode::Ionian);
        let p = generate_progression(key, 5, ProgressionStyle::BluesTwelveBar, 0);
        assert_eq!(p[4].root, PitchClass::F);
        assert_eq!(p[4].quality, ChordQuality::Dom7);
    }

    #[test]
    fn modal_dorian_starts_on_tonic_minor() {
        let key = Scale::new(PitchClass::D, Mode::Dorian);
        let p = generate_progression(key, 2, ProgressionStyle::ModalDorian, 0);
        assert_eq!(p[0].root, PitchClass::D);
        assert_eq!(p[0].quality, ChordQuality::Min);
    }

    #[test]
    fn random_progression_is_deterministic_with_seed() {
        let key = Scale::new(PitchClass::C, Mode::Ionian);
        let a = generate_progression(key, 16, ProgressionStyle::Random, 42);
        let b = generate_progression(key, 16, ProgressionStyle::Random, 42);
        assert_eq!(a, b);
    }

    #[test]
    fn random_progression_seeds_differ() {
        let key = Scale::new(PitchClass::C, Mode::Ionian);
        let a = generate_progression(key, 16, ProgressionStyle::Random, 1);
        let b = generate_progression(key, 16, ProgressionStyle::Random, 2);
        assert_ne!(a, b);
    }

    #[test]
    fn zero_length_yields_empty_progression() {
        let key = Scale::new(PitchClass::C, Mode::Ionian);
        let p = generate_progression(key, 0, ProgressionStyle::PopIVviIV, 0);
        assert!(p.is_empty());
    }
}
