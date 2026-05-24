//! Phase 6 (Self-verification) — deterministic post-render metrics
//! that decide whether the audio the LLM just composed is fit to
//! reach the listener. Used by the offline render-and-judge path
//! in [`render::render_and_judge`].
//!
//! These metrics are intentionally simple and fast (single-pass over
//! the rendered buffer). A richer aesthetic Critic is left to a
//! separate LLM pass — this module only catches *obvious* badness.

use crate::frame::StereoFrame;

/// Threshold below which a sample is treated as silent (≈ −90 dBFS).
const SILENCE_LINEAR: f32 = 3.16e-5;

/// Threshold at/above which we count a sample as clipped after the
/// safety limiter (the limiter ceiling is −1 dBFS by default).
const CLIP_LINEAR: f32 = 0.999;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Verdict {
    pub peak: f32,
    pub rms: f32,
    pub peak_db: f32,
    pub rms_db: f32,
    pub dynamic_range_db: f32,
    pub clipped_sample_ratio: f32,
    pub silence_ratio: f32,
    pub estimated_onset_rate_per_sec: f32,
    pub passed: bool,
    pub reasons: VerdictReasons,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerdictReasons {
    pub too_silent: bool,
    pub too_clipped: bool,
    pub too_flat: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct VerdictConfig {
    pub sample_rate: u32,
    /// Maximum allowed fraction of clipped samples (default 0.01 = 1 %).
    pub max_clip_ratio: f32,
    /// Maximum allowed fraction of silent samples (default 0.95 = 95 %).
    /// Anything above this is "essentially silent".
    pub max_silence_ratio: f32,
    /// Minimum dynamic range in dB (default 6 dB).
    pub min_dynamic_range_db: f32,
}

impl Default for VerdictConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            max_clip_ratio: 0.01,
            max_silence_ratio: 0.95,
            min_dynamic_range_db: 6.0,
        }
    }
}

pub fn judge(audio: &[StereoFrame], config: VerdictConfig) -> Verdict {
    let total = (audio.len() as f32).max(1.0);
    let mut peak = 0.0_f32;
    let mut sum_sq = 0.0_f64;
    let mut clipped = 0_u64;
    let mut silent = 0_u64;
    let mut onsets = 0_u64;
    let mut prev_energy: f32 = 0.0;
    let onset_threshold = 0.1; // sudden energy jump ≥ 0.1 amplitude
    for f in audio {
        let l = f.left.abs();
        let r = f.right.abs();
        let m = l.max(r);
        peak = peak.max(m);
        sum_sq += (f.left as f64 * f.left as f64) + (f.right as f64 * f.right as f64);
        if m >= CLIP_LINEAR {
            clipped += 1;
        }
        if m < SILENCE_LINEAR {
            silent += 1;
        }
        // Crude onset detection: amplitude jump.
        let energy = m;
        if energy - prev_energy > onset_threshold {
            onsets += 1;
        }
        prev_energy = energy;
    }
    let rms = ((sum_sq / (2.0 * total as f64)).sqrt()) as f32;
    let peak_db = lin_to_db(peak);
    let rms_db = lin_to_db(rms);
    let dynamic_range_db = peak_db - rms_db;
    let clipped_sample_ratio = clipped as f32 / total;
    let silence_ratio = silent as f32 / total;
    let duration_sec = total / config.sample_rate.max(1) as f32;
    let estimated_onset_rate_per_sec = if duration_sec > 0.0 {
        onsets as f32 / duration_sec
    } else {
        0.0
    };

    let too_silent = silence_ratio > config.max_silence_ratio;
    let too_clipped = clipped_sample_ratio > config.max_clip_ratio;
    let too_flat = !too_silent && dynamic_range_db < config.min_dynamic_range_db;
    let passed = !(too_silent || too_clipped || too_flat);

    Verdict {
        peak,
        rms,
        peak_db,
        rms_db,
        dynamic_range_db,
        clipped_sample_ratio,
        silence_ratio,
        estimated_onset_rate_per_sec,
        passed,
        reasons: VerdictReasons {
            too_silent,
            too_clipped,
            too_flat,
        },
    }
}

fn lin_to_db(linear: f32) -> f32 {
    if linear <= 1.0e-9 {
        -90.0
    } else {
        20.0 * linear.log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silence(n: usize) -> Vec<StereoFrame> {
        vec![StereoFrame::SILENCE; n]
    }

    fn sine(n: usize, freq: f32, sr: u32, amplitude: f32) -> Vec<StereoFrame> {
        (0..n)
            .map(|i| {
                let v =
                    amplitude * (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin();
                StereoFrame::new(v, v)
            })
            .collect()
    }

    #[test]
    fn silence_fails_for_too_silent() {
        let v = judge(&silence(48_000), VerdictConfig::default());
        assert!(!v.passed);
        assert!(v.reasons.too_silent);
        assert!(v.silence_ratio > 0.95);
    }

    #[test]
    fn loud_clipped_audio_fails_for_too_clipped() {
        // Half the buffer is loud sine at 1.0, the other half is silent.
        // Clip ratio will be > 1% so it fails for clipping.
        let mut audio = sine(24_000, 440.0, 48_000, 1.5); // amplitude > 1 → saturates after clamp
        audio.extend(silence(24_000));
        // Clamp to ±1 to simulate post-limiter saturation we want to detect.
        for f in &mut audio {
            f.left = f.left.clamp(-1.0, 1.0);
            f.right = f.right.clamp(-1.0, 1.0);
        }
        let v = judge(&audio, VerdictConfig::default());
        assert!(v.reasons.too_clipped, "expected too_clipped, got {v:?}");
        assert!(!v.passed);
    }

    #[test]
    fn moderate_sine_passes() {
        let audio = sine(48_000, 440.0, 48_000, 0.5);
        // Pure sine has only ~3 dB DR (peak vs RMS). Real music has
        // more. Relax the threshold for this single-tone test.
        let cfg = VerdictConfig {
            min_dynamic_range_db: 2.5,
            ..VerdictConfig::default()
        };
        let v = judge(&audio, cfg);
        assert!(v.passed, "expected passed verdict, got {v:?}");
        assert!(v.peak > 0.4 && v.peak < 0.51);
        assert!(!v.reasons.too_silent);
        assert!(!v.reasons.too_clipped);
    }

    #[test]
    fn estimated_onset_rate_for_silence_is_zero() {
        let v = judge(&silence(48_000), VerdictConfig::default());
        assert!(v.estimated_onset_rate_per_sec < 0.5);
    }

    #[test]
    fn dynamic_range_relates_peak_and_rms() {
        let audio = sine(48_000, 440.0, 48_000, 0.5);
        let v = judge(&audio, VerdictConfig::default());
        // For a pure sine: RMS = peak / sqrt(2), so peak_db - rms_db ≈ 3 dB
        // because both channels carry the same sine (their joint RMS
        // matches the mono RMS, so peak/rms ≈ √2).
        assert!(v.dynamic_range_db > 2.5 && v.dynamic_range_db < 4.0);
    }
}
