//! Internal mini-synth backend implementing the `SynthVoice` contract
//! defined in `docs/adr/002-synth-voice-trait.md`.
//!
//! Provides the `SynthVoice` trait plus two concrete implementations
//! (`SineAdsrVoice`, `SawAdsrVoice`) and a shared `AdsrEnvelope`. All
//! types are RT-safe: `note_on`, `note_off`, and `render` never
//! allocate, lock, log, or call into any blocking code.

use crate::frame::StereoFrame;

/// Samples with absolute value below this threshold are flushed to zero
/// to avoid subnormal CPU penalties.
const DENORMAL_THRESHOLD: f32 = 1.0e-15;

/// Polyphonic voice contract. See ADR-002.
///
/// `Send` is required so that voices can be moved into the audio runtime
/// at construction time. `Sync` is intentionally NOT required — voices
/// are owned exclusively by their containing source on the audio thread.
pub trait SynthVoice: Send {
    /// Start a new note with MIDI `pitch` (0..=127) and `velocity` (0..=127).
    /// Resets the envelope and replaces any currently sounding note on
    /// this voice instance.
    fn note_on(&mut self, pitch: u8, velocity: u8);

    /// Release the note matching `pitch`. No-op if the voice is sounding
    /// a different pitch. Triggers the release phase of the envelope.
    fn note_off(&mut self, pitch: u8);

    /// Overwrite `out` with the voice's stereo output for the next
    /// `out.len()` frames. An idle voice writes silence.
    fn render(&mut self, out: &mut [StereoFrame]);

    /// True when the envelope is in any state other than `Idle`.
    fn is_active(&self) -> bool;

    /// The pitch currently held by this voice, if any. Used by the
    /// sequencer for voice stealing.
    fn current_pitch(&self) -> Option<u8>;
}

/// ADSR envelope stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear ADSR envelope. All times are in milliseconds, sustain is a
/// 0..=1 amplitude level.
#[derive(Debug, Clone, Copy)]
pub struct AdsrEnvelope {
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain_level: f32,
    pub release_ms: f32,
    sample_rate: u32,
    stage: AdsrStage,
    level: f32,
    /// Fixed per-sample decrement during the release stage, computed
    /// from `level` at the moment `release()` is called so the segment
    /// is linear (not exponential).
    release_step: f32,
}

impl AdsrEnvelope {
    pub fn new(
        attack_ms: f32,
        decay_ms: f32,
        sustain_level: f32,
        release_ms: f32,
        sample_rate: u32,
    ) -> Self {
        Self {
            attack_ms: attack_ms.max(0.0),
            decay_ms: decay_ms.max(0.0),
            sustain_level: sustain_level.clamp(0.0, 1.0),
            release_ms: release_ms.max(0.0),
            sample_rate: sample_rate.max(1),
            stage: AdsrStage::Idle,
            level: 0.0,
            release_step: 0.0,
        }
    }

    pub fn trigger(&mut self) {
        self.stage = AdsrStage::Attack;
        // Restart from current level so re-triggers don't click.
    }

    pub fn release(&mut self) {
        if self.stage != AdsrStage::Idle {
            self.stage = AdsrStage::Release;
            let samples = (self.release_ms * 0.001 * self.sample_rate as f32).max(1.0);
            // Fixed linear step from the current level down to 0.
            self.release_step = (self.level / samples).max(DENORMAL_THRESHOLD);
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != AdsrStage::Idle
    }

    /// Advance one sample and return the current amplitude.
    pub fn next_sample(&mut self) -> f32 {
        match self.stage {
            AdsrStage::Idle => {
                self.level = 0.0;
                0.0
            }
            AdsrStage::Attack => {
                let step = step_for(self.attack_ms, self.sample_rate);
                self.level += step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = AdsrStage::Decay;
                }
                self.level
            }
            AdsrStage::Decay => {
                let span = (1.0 - self.sustain_level).max(0.0);
                let step = span * step_for(self.decay_ms, self.sample_rate);
                self.level -= step;
                if self.level <= self.sustain_level {
                    self.level = self.sustain_level;
                    self.stage = AdsrStage::Sustain;
                }
                self.level
            }
            AdsrStage::Sustain => {
                self.level = self.sustain_level;
                self.level
            }
            AdsrStage::Release => {
                self.level -= self.release_step;
                if self.level <= DENORMAL_THRESHOLD {
                    self.level = 0.0;
                    self.stage = AdsrStage::Idle;
                }
                self.level
            }
        }
    }

    #[cfg(test)]
    fn stage(&self) -> AdsrStage {
        self.stage
    }
}

/// Per-sample envelope step for a linear segment that should span
/// `duration_ms` milliseconds at `sample_rate`. Returns 1.0 for
/// zero-length segments (instant transition).
#[inline]
fn step_for(duration_ms: f32, sample_rate: u32) -> f32 {
    let samples = duration_ms * 0.001 * sample_rate as f32;
    if samples <= 1.0 {
        1.0
    } else {
        1.0 / samples
    }
}

/// MIDI pitch → frequency in Hz. Standard equal temperament: A4 (69)
/// = 440 Hz.
#[inline]
pub fn midi_to_freq_hz(pitch: u8) -> f64 {
    let p = pitch.min(127) as f64;
    440.0 * 2.0_f64.powf((p - 69.0) / 12.0)
}

/// Phase accumulator running in `[0.0, 1.0)` at `f64` precision.
#[derive(Debug, Clone, Copy)]
struct PhaseAccumulator {
    phase: f64,
    increment: f64,
}

impl PhaseAccumulator {
    fn new() -> Self {
        Self {
            phase: 0.0,
            increment: 0.0,
        }
    }

    fn set_frequency(&mut self, freq_hz: f64, sample_rate: u32) {
        if sample_rate == 0 {
            self.increment = 0.0;
        } else {
            self.increment = freq_hz / sample_rate as f64;
        }
    }

    fn advance(&mut self) -> f64 {
        let out = self.phase;
        self.phase += self.increment;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        out
    }

    fn reset(&mut self) {
        self.phase = 0.0;
    }
}

/// Sine + ADSR voice.
pub struct SineAdsrVoice {
    osc: PhaseAccumulator,
    env: AdsrEnvelope,
    pitch: Option<u8>,
    velocity_amp: f32,
    sample_rate: u32,
}

impl SineAdsrVoice {
    pub fn new(env: AdsrEnvelope, sample_rate: u32) -> Self {
        Self {
            osc: PhaseAccumulator::new(),
            env,
            pitch: None,
            velocity_amp: 0.0,
            sample_rate: sample_rate.max(1),
        }
    }
}

impl SynthVoice for SineAdsrVoice {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let pitch = pitch.min(127);
        self.pitch = Some(pitch);
        self.velocity_amp = velocity.min(127) as f32 / 127.0;
        self.osc
            .set_frequency(midi_to_freq_hz(pitch), self.sample_rate);
        self.osc.reset();
        self.env.trigger();
    }

    fn note_off(&mut self, pitch: u8) {
        if self.pitch == Some(pitch.min(127)) {
            self.env.release();
        }
    }

    fn render(&mut self, out: &mut [StereoFrame]) {
        for frame in out.iter_mut() {
            let amp = self.env.next_sample() * self.velocity_amp;
            let sample = (self.osc.advance() * std::f64::consts::TAU).sin() as f32 * amp;
            let sample = if sample.abs() < DENORMAL_THRESHOLD {
                0.0
            } else {
                sample
            };
            *frame = StereoFrame::new(sample, sample);
        }
        if !self.env.is_active() {
            self.pitch = None;
        }
    }

    fn is_active(&self) -> bool {
        self.env.is_active()
    }

    fn current_pitch(&self) -> Option<u8> {
        self.pitch
    }
}

/// Naive saw + ADSR voice. A polyBLEP / band-limited version is left as
/// a future improvement; this is the simplest correct implementation.
pub struct SawAdsrVoice {
    osc: PhaseAccumulator,
    env: AdsrEnvelope,
    pitch: Option<u8>,
    velocity_amp: f32,
    sample_rate: u32,
}

impl SawAdsrVoice {
    pub fn new(env: AdsrEnvelope, sample_rate: u32) -> Self {
        Self {
            osc: PhaseAccumulator::new(),
            env,
            pitch: None,
            velocity_amp: 0.0,
            sample_rate: sample_rate.max(1),
        }
    }
}

impl SynthVoice for SawAdsrVoice {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let pitch = pitch.min(127);
        self.pitch = Some(pitch);
        self.velocity_amp = velocity.min(127) as f32 / 127.0;
        self.osc
            .set_frequency(midi_to_freq_hz(pitch), self.sample_rate);
        self.osc.reset();
        self.env.trigger();
    }

    fn note_off(&mut self, pitch: u8) {
        if self.pitch == Some(pitch.min(127)) {
            self.env.release();
        }
    }

    fn render(&mut self, out: &mut [StereoFrame]) {
        for frame in out.iter_mut() {
            let amp = self.env.next_sample() * self.velocity_amp;
            // Naive saw in [-1.0, 1.0): 2 * phase - 1.
            let sample = ((self.osc.advance() * 2.0 - 1.0) as f32) * amp;
            let sample = if sample.abs() < DENORMAL_THRESHOLD {
                0.0
            } else {
                sample
            };
            *frame = StereoFrame::new(sample, sample);
        }
        if !self.env.is_active() {
            self.pitch = None;
        }
    }

    fn is_active(&self) -> bool {
        self.env.is_active()
    }

    fn current_pitch(&self) -> Option<u8> {
        self.pitch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn fast_env() -> AdsrEnvelope {
        AdsrEnvelope::new(1.0, 5.0, 0.7, 5.0, SR)
    }

    fn fill(voice: &mut dyn SynthVoice, n: usize) -> Vec<StereoFrame> {
        let mut buf = vec![StereoFrame::SILENCE; n];
        voice.render(&mut buf);
        buf
    }

    fn peak(buf: &[StereoFrame]) -> f32 {
        buf.iter().fold(0.0_f32, |m, f| m.max(f.left.abs()))
    }

    #[test]
    fn midi_to_freq_hz_matches_standard_equal_temperament() {
        assert!((midi_to_freq_hz(69) - 440.0).abs() < 0.001, "A4");
        assert!(
            (midi_to_freq_hz(60) - 261.625_565_300_598_6).abs() < 0.001,
            "C4 (middle C)"
        );
        assert!(
            (midi_to_freq_hz(72) - 523.251_130_601_197).abs() < 0.001,
            "C5"
        );
        assert!(
            midi_to_freq_hz(0) > 8.0 && midi_to_freq_hz(0) < 9.0,
            "MIDI 0 ≈ 8.176 Hz"
        );
        assert!(
            midi_to_freq_hz(127) > 12_000.0 && midi_to_freq_hz(127) < 13_000.0,
            "MIDI 127 ≈ 12544 Hz"
        );
    }

    #[test]
    fn idle_voice_has_no_pitch_and_renders_silence() {
        let mut voice = SineAdsrVoice::new(fast_env(), SR);
        assert_eq!(voice.current_pitch(), None);
        assert!(!voice.is_active());
        let buf = fill(&mut voice, 64);
        assert_eq!(peak(&buf), 0.0);
    }

    #[test]
    fn note_on_makes_voice_active_with_pitch() {
        let mut voice = SineAdsrVoice::new(fast_env(), SR);
        voice.note_on(69, 100);
        assert!(voice.is_active());
        assert_eq!(voice.current_pitch(), Some(69));
    }

    #[test]
    fn note_off_for_wrong_pitch_is_a_no_op() {
        let mut voice = SineAdsrVoice::new(fast_env(), SR);
        voice.note_on(69, 100);
        // Drain a few samples so envelope advances past Idle.
        let _ = fill(&mut voice, 1);
        voice.note_off(60); // wrong pitch
        assert!(voice.is_active(), "should still be active");
    }

    #[test]
    fn lifecycle_settles_to_idle_after_release_completes() {
        let mut voice = SineAdsrVoice::new(fast_env(), SR);
        voice.note_on(69, 127);
        // Render through attack + decay + a bit of sustain.
        let _ = fill(&mut voice, SR as usize / 10); // 100ms
        voice.note_off(69);
        // Render enough to outlast 5ms release.
        let _ = fill(&mut voice, SR as usize / 10);
        assert!(!voice.is_active(), "should be idle after release");
        assert_eq!(voice.current_pitch(), None, "pitch cleared on idle");
    }

    #[test]
    fn sine_a4_full_velocity_renders_audible_signal() {
        let mut voice = SineAdsrVoice::new(fast_env(), SR);
        voice.note_on(69, 127);
        let buf = fill(&mut voice, SR as usize / 10); // 100ms
        assert!(
            peak(&buf) > 0.5,
            "expected loud A4, got peak {}",
            peak(&buf)
        );
        // Verify both stereo channels are identical (mono → stereo replication).
        for f in &buf {
            assert!((f.left - f.right).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn saw_a4_full_velocity_renders_audible_signal() {
        let mut voice = SawAdsrVoice::new(fast_env(), SR);
        voice.note_on(69, 127);
        let buf = fill(&mut voice, SR as usize / 10);
        assert!(
            peak(&buf) > 0.5,
            "expected loud saw, got peak {}",
            peak(&buf)
        );
    }

    #[test]
    fn output_stays_within_unit_range() {
        let mut voice = SawAdsrVoice::new(fast_env(), SR);
        voice.note_on(72, 127);
        let buf = fill(&mut voice, SR as usize / 20);
        for f in &buf {
            assert!(f.left.abs() <= 1.0 && f.right.abs() <= 1.0);
        }
    }

    #[test]
    fn velocity_scales_amplitude() {
        let mut loud = SineAdsrVoice::new(fast_env(), SR);
        let mut quiet = SineAdsrVoice::new(fast_env(), SR);
        loud.note_on(69, 127);
        quiet.note_on(69, 32);
        let buf_loud = fill(&mut loud, SR as usize / 10);
        let buf_quiet = fill(&mut quiet, SR as usize / 10);
        assert!(
            peak(&buf_loud) > peak(&buf_quiet) * 2.0,
            "loud {} should be > 2x quiet {}",
            peak(&buf_loud),
            peak(&buf_quiet)
        );
    }

    #[test]
    fn adsr_envelope_trajectory_visits_all_stages_in_order() {
        let mut env = AdsrEnvelope::new(2.0, 4.0, 0.5, 4.0, SR);
        assert_eq!(env.stage(), AdsrStage::Idle);
        env.trigger();
        assert_eq!(env.stage(), AdsrStage::Attack);
        // Attack reaches 1.0 in ~96 samples (2ms at 48kHz).
        for _ in 0..200 {
            env.next_sample();
        }
        assert!(
            matches!(env.stage(), AdsrStage::Decay | AdsrStage::Sustain),
            "should have progressed past attack"
        );
        // Drive through decay into sustain.
        for _ in 0..400 {
            env.next_sample();
        }
        assert_eq!(env.stage(), AdsrStage::Sustain);
        let sustain_sample = env.next_sample();
        assert!(
            (sustain_sample - 0.5).abs() < 0.01,
            "sustain ≈ 0.5, got {sustain_sample}"
        );
        env.release();
        assert_eq!(env.stage(), AdsrStage::Release);
        for _ in 0..400 {
            env.next_sample();
        }
        assert_eq!(env.stage(), AdsrStage::Idle, "release completes");
    }

    #[test]
    fn release_from_attack_still_settles_to_idle() {
        let mut env = AdsrEnvelope::new(50.0, 50.0, 0.5, 5.0, SR);
        env.trigger();
        for _ in 0..10 {
            env.next_sample(); // mid-attack
        }
        env.release();
        for _ in 0..(SR as usize / 10) {
            env.next_sample();
        }
        assert!(
            !env.is_active(),
            "should idle even when released mid-attack"
        );
    }

    #[test]
    fn denormal_samples_are_flushed_to_zero() {
        // Construct an envelope nearly drained.
        let mut env = AdsrEnvelope::new(0.0, 0.0, 0.0, 0.0, SR);
        env.trigger();
        // Run a few samples — with all-zero times the envelope should
        // settle to release/idle almost immediately.
        for _ in 0..8 {
            env.next_sample();
        }
        let mut voice = SineAdsrVoice::new(env, SR);
        voice.note_on(69, 1);
        let buf = fill(&mut voice, 64);
        for f in &buf {
            assert!(f.left == 0.0 || f.left.abs() >= DENORMAL_THRESHOLD);
        }
    }

    #[test]
    fn voice_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<SineAdsrVoice>();
        assert_send::<SawAdsrVoice>();
        assert_send::<Box<dyn SynthVoice>>();
    }

    #[test]
    fn sine_a4_dominant_frequency_is_close_to_440hz() {
        // Naive DFT bin check at 440Hz vs neighbors.
        let mut voice = SineAdsrVoice::new(AdsrEnvelope::new(0.5, 0.5, 1.0, 5.0, SR), SR);
        voice.note_on(69, 127);
        let mut buf = vec![StereoFrame::SILENCE; SR as usize / 4]; // 250ms
        voice.render(&mut buf);
        let mono: Vec<f32> = buf.iter().map(|f| f.left).collect();
        let bin_at_440 = goertzel(&mono, 440.0, SR);
        let bin_at_220 = goertzel(&mono, 220.0, SR);
        let bin_at_880 = goertzel(&mono, 880.0, SR);
        assert!(
            bin_at_440 > bin_at_220 * 5.0,
            "440 ({bin_at_440}) should dominate 220 ({bin_at_220})"
        );
        assert!(
            bin_at_440 > bin_at_880 * 5.0,
            "440 ({bin_at_440}) should dominate 880 ({bin_at_880})"
        );
    }

    /// Goertzel single-bin magnitude — minimal FFT alternative for one
    /// frequency.
    fn goertzel(samples: &[f32], target_hz: f32, sample_rate: u32) -> f32 {
        let n = samples.len() as f32;
        let k = (target_hz * n / sample_rate as f32).round();
        let w = std::f32::consts::TAU * k / n;
        let cosine = w.cos();
        let coeff = 2.0 * cosine;
        let mut q1 = 0.0;
        let mut q2 = 0.0;
        for &x in samples {
            let q0 = coeff * q1 - q2 + x;
            q2 = q1;
            q1 = q0;
        }
        (q1 * q1 + q2 * q2 - q1 * q2 * coeff).sqrt() / n
    }
}
