use serde::{Deserialize, Serialize};

/// Ticks per quarter note (PPQ). Fixed at 480 to match common MIDI tooling.
pub const TICKS_PER_QUARTER: u32 = 480;

/// A musical time signature such as 4/4, 3/4, or 6/8.
///
/// `denominator` MUST be a positive power of two (1, 2, 4, 8, 16, 32). The
/// `new` constructor validates this; the `FOUR_FOUR` constant is provided
/// for the common case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl TimeSignature {
    /// 4/4 — the most common time signature, used as default.
    pub const FOUR_FOUR: Self = Self {
        numerator: 4,
        denominator: 4,
    };

    /// Construct a time signature. Returns `None` if `numerator == 0` or if
    /// `denominator` is not a positive power of two in `1..=32`.
    pub const fn new(numerator: u8, denominator: u8) -> Option<Self> {
        if numerator == 0 {
            return None;
        }
        match denominator {
            1 | 2 | 4 | 8 | 16 | 32 => Some(Self {
                numerator,
                denominator,
            }),
            _ => None,
        }
    }

    /// Ticks in one beat of this time signature.
    pub const fn ticks_per_beat(self) -> u32 {
        TICKS_PER_QUARTER * 4 / self.denominator as u32
    }

    /// Ticks in one bar of this time signature.
    pub const fn ticks_per_bar(self) -> u32 {
        self.numerator as u32 * self.ticks_per_beat()
    }
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self::FOUR_FOUR
    }
}

/// A point in musical time as `{ bar, beat, tick }`.
///
/// `beat` and `tick` are always relative to the current bar / beat under
/// the active `TimeSignature`. Construct values via
/// [`MusicalTime::from_total_ticks`] when you have a tick count and want
/// the bar/beat decomposition.
/// Order: lexicographic by `(bar, beat, tick)`. The derive on
/// `PartialOrd` / `Ord` gives the musically meaningful comparison
/// because the struct fields are listed in coarse-to-fine order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MusicalTime {
    pub bar: u32,
    pub beat: u16,
    pub tick: u16,
}

impl MusicalTime {
    /// Bar 0, beat 0, tick 0 — the start of the timeline.
    pub const ZERO: Self = Self {
        bar: 0,
        beat: 0,
        tick: 0,
    };

    /// Construct from explicit bar/beat/tick components. No validation is
    /// performed; out-of-range beats or ticks will be normalized by the
    /// caller (or surface as wrong frame positions on conversion).
    pub const fn new(bar: u32, beat: u16, tick: u16) -> Self {
        Self { bar, beat, tick }
    }

    /// Total ticks since the timeline origin under the given time signature.
    pub fn to_total_ticks(self, ts: TimeSignature) -> u64 {
        let bar_ticks = ts.ticks_per_bar() as u64;
        let beat_ticks = ts.ticks_per_beat() as u64;
        (self.bar as u64) * bar_ticks + (self.beat as u64) * beat_ticks + (self.tick as u64)
    }

    /// Decompose a total tick count into bar/beat/tick under the given
    /// time signature.
    pub fn from_total_ticks(total: u64, ts: TimeSignature) -> Self {
        let bar_ticks = ts.ticks_per_bar() as u64;
        let beat_ticks = ts.ticks_per_beat() as u64;
        debug_assert!(bar_ticks > 0 && beat_ticks > 0);
        let bar = (total / bar_ticks) as u32;
        let rem = total % bar_ticks;
        let beat = (rem / beat_ticks) as u16;
        let tick = (rem % beat_ticks) as u16;
        Self { bar, beat, tick }
    }
}

impl Default for MusicalTime {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Transport state — BPM, time signature, and swing amount.
///
/// `bpm` is in quarter notes per minute and must be positive (typically
/// 30.0..=300.0). `swing` is in `0.0..=1.0` and is stored but not yet
/// applied by the protocol; downstream consumers may interpret it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transport {
    pub bpm: f32,
    pub time_signature: TimeSignature,
    /// Stored for future use; the conversion functions in this module
    /// do NOT apply swing. Downstream consumers (sequencers, quantizers
    /// in later phases) may interpret it.
    pub swing: f32,
}

impl Transport {
    pub const fn new(bpm: f32, time_signature: TimeSignature) -> Self {
        Self {
            bpm,
            time_signature,
            swing: 0.0,
        }
    }

    pub const fn with_swing(self, swing: f32) -> Self {
        Self { swing, ..self }
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new(120.0, TimeSignature::FOUR_FOUR)
    }
}

/// Frames between adjacent ticks at the given BPM and sample rate.
#[inline]
fn frames_per_tick(bpm: f32, sample_rate: u32) -> f64 {
    let bpm = bpm as f64;
    let sr = sample_rate as f64;
    let tpq = TICKS_PER_QUARTER as f64;
    60.0 * sr / (bpm * tpq)
}

/// Convert a total tick count into frames at the given transport.
#[inline]
fn ticks_to_frames(total_ticks: u64, transport: Transport, sample_rate: u32) -> u64 {
    if transport.bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let fpt = frames_per_tick(transport.bpm, sample_rate);
    (total_ticks as f64 * fpt).round() as u64
}

/// Convert a frame count into a total tick count at the given transport.
///
/// Uses `floor` (not `round`) so that the reported musical position is
/// never ahead of the actual frame — i.e. `frame_to_musical_time` never
/// overshoots a tick that the engine has not yet reached. This matters
/// for downstream scheduling (Phase 1c) where a "what musical time are
/// we at?" query must not trigger an action one tick early.
#[inline]
fn frames_to_ticks(frames: u64, transport: Transport, sample_rate: u32) -> u64 {
    if transport.bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let fpt = frames_per_tick(transport.bpm, sample_rate);
    if fpt <= 0.0 {
        return 0;
    }
    (frames as f64 / fpt).floor() as u64
}

/// Convert a `MusicalTime` into an absolute engine frame, where `origin_frame`
/// is the engine frame at which the transport last started.
pub fn musical_time_to_frame(
    t: MusicalTime,
    transport: Transport,
    origin_frame: u64,
    sample_rate: u32,
) -> u64 {
    let total_ticks = t.to_total_ticks(transport.time_signature);
    let offset = ticks_to_frames(total_ticks, transport, sample_rate);
    origin_frame.saturating_add(offset)
}

/// Convert an absolute engine `frame` into a `MusicalTime`, relative to
/// `origin_frame`. Frames before the origin clamp to `MusicalTime::ZERO`.
pub fn frame_to_musical_time(
    frame: u64,
    transport: Transport,
    origin_frame: u64,
    sample_rate: u32,
) -> MusicalTime {
    let offset = frame.saturating_sub(origin_frame);
    let total_ticks = frames_to_ticks(offset, transport, sample_rate);
    MusicalTime::from_total_ticks(total_ticks, transport.time_signature)
}

/// Round `frame` up to the next bar boundary. If `frame` is already on a
/// bar boundary, returns `frame` unchanged.
pub fn quantize_to_next_bar(
    frame: u64,
    transport: Transport,
    origin_frame: u64,
    sample_rate: u32,
) -> u64 {
    let ticks_per_bar = transport.time_signature.ticks_per_bar() as u64;
    let frames_per_bar = ticks_to_frames(ticks_per_bar, transport, sample_rate);
    quantize_up(frame, origin_frame, frames_per_bar)
}

/// Round `frame` up to the next beat boundary. If `frame` is already on a
/// beat boundary, returns `frame` unchanged.
pub fn quantize_to_next_beat(
    frame: u64,
    transport: Transport,
    origin_frame: u64,
    sample_rate: u32,
) -> u64 {
    let ticks_per_beat = transport.time_signature.ticks_per_beat() as u64;
    let frames_per_beat = ticks_to_frames(ticks_per_beat, transport, sample_rate);
    quantize_up(frame, origin_frame, frames_per_beat)
}

#[inline]
fn quantize_up(frame: u64, origin_frame: u64, step: u64) -> u64 {
    if step == 0 {
        return frame;
    }
    let offset = frame.saturating_sub(origin_frame);
    let units = offset / step;
    let rem = offset % step;
    let target_units = if rem == 0 { units } else { units + 1 };
    origin_frame.saturating_add(target_units.saturating_mul(step))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn t44(bpm: f32) -> Transport {
        Transport::new(bpm, TimeSignature::FOUR_FOUR)
    }

    #[test]
    fn time_signature_constructor_rejects_invalid_inputs() {
        assert!(TimeSignature::new(0, 4).is_none(), "zero numerator");
        assert!(TimeSignature::new(4, 0).is_none(), "zero denominator");
        assert!(TimeSignature::new(4, 3).is_none(), "non power-of-two");
        assert!(TimeSignature::new(4, 64).is_none(), "too large denominator");
        assert_eq!(TimeSignature::new(4, 4), Some(TimeSignature::FOUR_FOUR));
        assert!(TimeSignature::new(6, 8).is_some());
        assert!(TimeSignature::new(3, 4).is_some());
    }

    #[test]
    fn time_signature_ticks_per_beat_and_bar() {
        let four_four = TimeSignature::FOUR_FOUR;
        assert_eq!(four_four.ticks_per_beat(), 480);
        assert_eq!(four_four.ticks_per_bar(), 4 * 480);

        let six_eight = TimeSignature::new(6, 8).unwrap();
        assert_eq!(six_eight.ticks_per_beat(), 240, "eighth note = 240 ticks");
        assert_eq!(six_eight.ticks_per_bar(), 6 * 240);

        let three_four = TimeSignature::new(3, 4).unwrap();
        assert_eq!(three_four.ticks_per_beat(), 480);
        assert_eq!(three_four.ticks_per_bar(), 3 * 480);
    }

    #[test]
    fn musical_time_total_ticks_round_trip() {
        let cases = [
            TimeSignature::FOUR_FOUR,
            TimeSignature::new(3, 4).unwrap(),
            TimeSignature::new(6, 8).unwrap(),
            TimeSignature::new(5, 4).unwrap(),
        ];
        for ts in cases {
            for bar in 0..50 {
                for beat in 0..(ts.numerator as u16) {
                    for tick in (0..ts.ticks_per_beat() as u16).step_by(31) {
                        let mt = MusicalTime { bar, beat, tick };
                        let total = mt.to_total_ticks(ts);
                        let back = MusicalTime::from_total_ticks(total, ts);
                        assert_eq!(mt, back, "round-trip failed for {ts:?} {mt:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn musical_time_to_frame_at_120bpm_4_4() {
        let t = t44(120.0);
        assert_eq!(musical_time_to_frame(MusicalTime::ZERO, t, 0, SR), 0);
        // 1 beat @ 120 BPM = 0.5s = 24_000 frames @ 48 kHz
        assert_eq!(
            musical_time_to_frame(MusicalTime::new(0, 1, 0), t, 0, SR),
            24_000
        );
        // 1 bar @ 120 BPM in 4/4 = 4 beats = 2s = 96_000 frames
        assert_eq!(
            musical_time_to_frame(MusicalTime::new(1, 0, 0), t, 0, SR),
            96_000
        );
        // tick-level: 1 tick @ 120 BPM = 24_000 / 480 = 50 frames
        assert_eq!(
            musical_time_to_frame(MusicalTime::new(0, 0, 1), t, 0, SR),
            50
        );
    }

    #[test]
    fn frame_to_musical_time_at_120bpm_4_4() {
        let t = t44(120.0);
        assert_eq!(frame_to_musical_time(0, t, 0, SR), MusicalTime::ZERO);
        assert_eq!(
            frame_to_musical_time(48_000, t, 0, SR),
            MusicalTime::new(0, 2, 0),
            "1 second @ 120 BPM = beat 2"
        );
        assert_eq!(
            frame_to_musical_time(96_000, t, 0, SR),
            MusicalTime::new(1, 0, 0)
        );
        assert_eq!(
            frame_to_musical_time(50, t, 0, SR),
            MusicalTime::new(0, 0, 1)
        );
    }

    #[test]
    fn frame_to_musical_time_respects_origin_frame() {
        let t = t44(120.0);
        let origin = 1_000_000;
        assert_eq!(
            frame_to_musical_time(origin, t, origin, SR),
            MusicalTime::ZERO
        );
        assert_eq!(
            frame_to_musical_time(origin + 96_000, t, origin, SR),
            MusicalTime::new(1, 0, 0)
        );
        // Frames before origin clamp to ZERO.
        assert_eq!(
            frame_to_musical_time(origin - 1, t, origin, SR),
            MusicalTime::ZERO
        );
    }

    #[test]
    fn musical_time_to_frame_at_60bpm_3_4() {
        let t = Transport::new(60.0, TimeSignature::new(3, 4).unwrap());
        // 1 beat @ 60 BPM = 1s = 48_000 frames
        assert_eq!(
            musical_time_to_frame(MusicalTime::new(0, 1, 0), t, 0, SR),
            48_000
        );
        // 1 bar (3 beats) @ 60 BPM = 3s = 144_000 frames
        assert_eq!(
            musical_time_to_frame(MusicalTime::new(1, 0, 0), t, 0, SR),
            144_000
        );
    }

    #[test]
    fn round_trip_musical_to_frame_to_musical_clean_bpms() {
        // BPMs where frames_per_tick is exactly integer at 48 kHz
        // (so the conversion is lossless).
        // fpt = 60 * SR / (bpm * TICKS_PER_QUARTER) = 6000 / bpm
        // Integer when bpm divides 6000: 50, 60, 75, 100, 120, 125, 150, 200, 250.
        let bpms = [60.0_f32, 100.0, 120.0, 125.0, 150.0];
        for bpm in bpms {
            let t = t44(bpm);
            for bar in 0..80 {
                for beat in 0..4 {
                    for tick in (0..480_u16).step_by(37) {
                        let mt = MusicalTime { bar, beat, tick };
                        let frame = musical_time_to_frame(mt, t, 0, SR);
                        let back = frame_to_musical_time(frame, t, 0, SR);
                        assert_eq!(mt, back, "round-trip failed at bpm={bpm} {mt:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn round_trip_dirty_bpm_within_one_tick() {
        // 140 BPM at 48 kHz: fpt ≈ 42.857 (not integer). Round-trip
        // should still land within one tick of the original.
        let t = t44(140.0);
        for bar in 0..20 {
            for beat in 0..4 {
                for tick in (0..480_u16).step_by(53) {
                    let mt = MusicalTime { bar, beat, tick };
                    let frame = musical_time_to_frame(mt, t, 0, SR);
                    let back = frame_to_musical_time(frame, t, 0, SR);
                    let orig_total = mt.to_total_ticks(TimeSignature::FOUR_FOUR) as i64;
                    let back_total = back.to_total_ticks(TimeSignature::FOUR_FOUR) as i64;
                    assert!(
                        (orig_total - back_total).abs() <= 1,
                        "round-trip drifted >1 tick at bpm=140 {mt:?} → {back:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn quantize_to_next_bar_at_bar_boundary_is_identity() {
        let t = t44(120.0);
        // bar 0 starts at frame 0; bar 1 at frame 96_000
        assert_eq!(quantize_to_next_bar(0, t, 0, SR), 0);
        assert_eq!(quantize_to_next_bar(96_000, t, 0, SR), 96_000);
        assert_eq!(quantize_to_next_bar(192_000, t, 0, SR), 192_000);
    }

    #[test]
    fn quantize_to_next_bar_rounds_up_mid_bar_frames() {
        let t = t44(120.0);
        assert_eq!(quantize_to_next_bar(1, t, 0, SR), 96_000);
        assert_eq!(quantize_to_next_bar(95_999, t, 0, SR), 96_000);
        assert_eq!(quantize_to_next_bar(96_001, t, 0, SR), 192_000);
    }

    #[test]
    fn quantize_to_next_beat_at_beat_boundary_is_identity() {
        let t = t44(120.0);
        // beat is 24_000 frames at 120 BPM
        assert_eq!(quantize_to_next_beat(0, t, 0, SR), 0);
        assert_eq!(quantize_to_next_beat(24_000, t, 0, SR), 24_000);
        assert_eq!(quantize_to_next_beat(48_000, t, 0, SR), 48_000);
    }

    #[test]
    fn quantize_to_next_beat_rounds_up_off_beat_frames() {
        let t = t44(120.0);
        assert_eq!(quantize_to_next_beat(1, t, 0, SR), 24_000);
        assert_eq!(quantize_to_next_beat(23_999, t, 0, SR), 24_000);
        assert_eq!(quantize_to_next_beat(24_001, t, 0, SR), 48_000);
    }

    #[test]
    fn quantize_helpers_respect_origin_frame() {
        let t = t44(120.0);
        let origin = 1_000_000;
        assert_eq!(quantize_to_next_bar(origin, t, origin, SR), origin);
        assert_eq!(
            quantize_to_next_bar(origin + 1, t, origin, SR),
            origin + 96_000
        );
        assert_eq!(quantize_to_next_beat(origin, t, origin, SR), origin);
        assert_eq!(
            quantize_to_next_beat(origin + 1, t, origin, SR),
            origin + 24_000
        );
    }

    #[test]
    fn json_round_trip_for_protocol_types() {
        let ts = TimeSignature::new(6, 8).unwrap();
        let mt = MusicalTime::new(7, 2, 123);
        let transport = Transport::new(140.0, ts).with_swing(0.25);

        let s = serde_json::to_string(&ts).unwrap();
        let back: TimeSignature = serde_json::from_str(&s).unwrap();
        assert_eq!(back, ts);

        let s = serde_json::to_string(&mt).unwrap();
        let back: MusicalTime = serde_json::from_str(&s).unwrap();
        assert_eq!(back, mt);

        let s = serde_json::to_string(&transport).unwrap();
        let back: Transport = serde_json::from_str(&s).unwrap();
        assert_eq!(back, transport);
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        let zero_bpm = Transport {
            bpm: 0.0,
            time_signature: TimeSignature::FOUR_FOUR,
            swing: 0.0,
        };
        assert_eq!(
            musical_time_to_frame(MusicalTime::new(1, 0, 0), zero_bpm, 0, SR),
            0
        );
        assert_eq!(
            frame_to_musical_time(48_000, zero_bpm, 0, SR),
            MusicalTime::ZERO
        );
        assert_eq!(quantize_to_next_bar(123, zero_bpm, 0, SR), 123);
        assert_eq!(quantize_to_next_beat(123, zero_bpm, 0, SR), 123);

        let t = t44(120.0);
        assert_eq!(musical_time_to_frame(MusicalTime::ZERO, t, 0, 0), 0);
        assert_eq!(frame_to_musical_time(48_000, t, 0, 0), MusicalTime::ZERO);
    }
}
