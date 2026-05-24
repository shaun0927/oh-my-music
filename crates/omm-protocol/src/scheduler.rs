use serde::{Deserialize, Serialize};

use crate::musical_time::{
    musical_time_to_frame, quantize_to_next_bar, quantize_to_next_beat, MusicalTime, Transport,
};

pub const PLANNED_ACTION_MIN_LEAD_MS: u64 = 30_000;

/// How a scheduling request expresses *when* the action should fire.
///
/// The control side normalizes this into an absolute engine frame via
/// [`resolve_trigger`] using the runtime's current `Transport` and
/// `transport_start_frame`. The scheduler itself only ever sees frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleTrigger {
    /// Absolute engine frame. Original API.
    Frame(u64),
    /// Specific musical time relative to the transport.
    MusicalTime(MusicalTime),
    /// `bars` and `beats` after the current engine frame, using the
    /// active transport's tempo.
    RelativeMusical { bars: u32, beats: u16 },
    /// The next bar boundary (or the current frame, if exactly on a
    /// boundary).
    NextBarBoundary,
    /// The next beat boundary (or the current frame, if exactly on a
    /// boundary).
    NextBeatBoundary,
}

/// Convert a [`ScheduleTrigger`] into an absolute engine frame using
/// the active transport and engine clock. Pure function — no scheduler
/// or runtime state mutated. Safe to call from the control side.
pub fn resolve_trigger(
    trigger: ScheduleTrigger,
    transport: Transport,
    engine_now_frame: u64,
    transport_start_frame: u64,
    sample_rate: u32,
) -> u64 {
    match trigger {
        ScheduleTrigger::Frame(f) => f,
        ScheduleTrigger::MusicalTime(mt) => {
            musical_time_to_frame(mt, transport, transport_start_frame, sample_rate)
        }
        ScheduleTrigger::RelativeMusical { bars, beats } => {
            let mt = MusicalTime::new(bars, beats, 0);
            let mt_frames =
                musical_time_to_frame(mt, transport, transport_start_frame, sample_rate)
                    .saturating_sub(transport_start_frame);
            engine_now_frame.saturating_add(mt_frames)
        }
        ScheduleTrigger::NextBarBoundary => quantize_to_next_bar(
            engine_now_frame,
            transport,
            transport_start_frame,
            sample_rate,
        ),
        ScheduleTrigger::NextBeatBoundary => quantize_to_next_beat(
            engine_now_frame,
            transport,
            transport_start_frame,
            sample_rate,
        ),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ScheduledActionId(String);

impl ScheduledActionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ScheduledActionId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ScheduledActionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionOrigin {
    PlannedLlm,
    PlannedPi,
    Manual,
    Test,
    Emergency,
}

impl ActionOrigin {
    pub fn requires_planned_lead_time(self) -> bool {
        matches!(self, Self::PlannedLlm | Self::PlannedPi)
    }

    pub fn may_execute_immediately(self) -> bool {
        !self.requires_planned_lead_time()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineTime {
    pub frame: u64,
    pub sample_rate: u32,
}

impl EngineTime {
    pub fn new(frame: u64, sample_rate: u32) -> Self {
        Self {
            frame,
            sample_rate: sample_rate.max(1),
        }
    }

    pub fn frame_after_ms(&self, delta_ms: u64) -> u64 {
        self.frame
            .saturating_add(frames_for_duration_ms(delta_ms, self.sample_rate))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleRequestTiming {
    pub submitted_at_frame: u64,
    pub trigger_frame: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleValidation {
    pub action_id: ScheduledActionId,
    pub origin: ActionOrigin,
    pub timing: ScheduleRequestTiming,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleValidationError {
    #[error("scheduled action id must not be empty")]
    EmptyActionId,
    #[error(
        "planned action requires trigger_frame >= {minimum_trigger_frame}, got {trigger_frame}"
    )]
    PlannedActionTooSoon {
        trigger_frame: u64,
        minimum_trigger_frame: u64,
    },
}

pub fn validate_schedule_request(
    action_id: ScheduledActionId,
    origin: ActionOrigin,
    timing: ScheduleRequestTiming,
    sample_rate: u32,
) -> Result<ScheduleValidation, ScheduleValidationError> {
    if action_id.as_str().is_empty() {
        return Err(ScheduleValidationError::EmptyActionId);
    }

    if origin.requires_planned_lead_time() {
        let minimum_trigger_frame = EngineTime::new(timing.submitted_at_frame, sample_rate)
            .frame_after_ms(PLANNED_ACTION_MIN_LEAD_MS);
        if timing.trigger_frame < minimum_trigger_frame {
            return Err(ScheduleValidationError::PlannedActionTooSoon {
                trigger_frame: timing.trigger_frame,
                minimum_trigger_frame,
            });
        }
    }

    Ok(ScheduleValidation {
        action_id,
        origin,
        timing,
    })
}

pub fn frames_for_duration_ms(duration_ms: u64, sample_rate: u32) -> u64 {
    let numerator = duration_ms as u128 * sample_rate.max(1) as u128;
    numerator.div_ceil(1_000).min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 48_000;

    fn timing(trigger_frame: u64) -> ScheduleRequestTiming {
        ScheduleRequestTiming {
            submitted_at_frame: 100,
            trigger_frame,
        }
    }

    #[test]
    fn planned_actions_require_thirty_second_lead_time() {
        let minimum = EngineTime::new(100, SAMPLE_RATE).frame_after_ms(30_000);

        let err = validate_schedule_request(
            ScheduledActionId::new("planned-early"),
            ActionOrigin::PlannedLlm,
            timing(minimum - 1),
            SAMPLE_RATE,
        )
        .expect_err("planned action before the minimum lead time must be rejected");

        assert_eq!(
            err,
            ScheduleValidationError::PlannedActionTooSoon {
                trigger_frame: minimum - 1,
                minimum_trigger_frame: minimum,
            }
        );

        assert!(validate_schedule_request(
            ScheduledActionId::new("planned-ok"),
            ActionOrigin::PlannedPi,
            timing(minimum),
            SAMPLE_RATE,
        )
        .is_ok());
    }

    #[test]
    fn immediate_origins_can_execute_at_or_before_now() {
        for origin in [
            ActionOrigin::Manual,
            ActionOrigin::Test,
            ActionOrigin::Emergency,
        ] {
            let accepted = validate_schedule_request(
                ScheduledActionId::new(format!("{origin:?}")),
                origin,
                ScheduleRequestTiming {
                    submitted_at_frame: 1_000,
                    trigger_frame: 0,
                },
                SAMPLE_RATE,
            )
            .expect("immediate origin accepted");

            assert_eq!(accepted.origin, origin);
        }
    }

    #[test]
    fn duration_to_frames_rounds_up() {
        assert_eq!(frames_for_duration_ms(30_000, SAMPLE_RATE), 1_440_000);
        assert_eq!(frames_for_duration_ms(1, 44_100), 45);
    }

    fn t120() -> crate::Transport {
        crate::Transport::new(120.0, crate::TimeSignature::FOUR_FOUR)
    }

    #[test]
    fn resolve_trigger_frame_is_identity() {
        assert_eq!(
            resolve_trigger(ScheduleTrigger::Frame(12345), t120(), 0, 0, SAMPLE_RATE),
            12345
        );
    }

    #[test]
    fn resolve_trigger_musical_time_at_120bpm_4_4() {
        // bar 1 beat 0 tick 0 @ 120 BPM / 4-4 = 96000 frames from start
        let f = resolve_trigger(
            ScheduleTrigger::MusicalTime(MusicalTime::new(1, 0, 0)),
            t120(),
            0,
            0,
            SAMPLE_RATE,
        );
        assert_eq!(f, 96_000);
    }

    #[test]
    fn resolve_trigger_relative_musical_advances_from_engine_now() {
        // 16 bars @ 120 BPM = 32s = 1_536_000 frames
        let engine_now = 100_000;
        let f = resolve_trigger(
            ScheduleTrigger::RelativeMusical { bars: 16, beats: 0 },
            t120(),
            engine_now,
            0,
            SAMPLE_RATE,
        );
        assert_eq!(f, engine_now + 1_536_000);
    }

    #[test]
    fn resolve_trigger_next_bar_boundary_at_bar_returns_same_frame() {
        // engine_now = 96000 (exact bar 1 boundary) → unchanged
        let f = resolve_trigger(
            ScheduleTrigger::NextBarBoundary,
            t120(),
            96_000,
            0,
            SAMPLE_RATE,
        );
        assert_eq!(f, 96_000);
    }

    #[test]
    fn resolve_trigger_next_bar_boundary_mid_bar_rounds_up() {
        let f = resolve_trigger(ScheduleTrigger::NextBarBoundary, t120(), 1, 0, SAMPLE_RATE);
        assert_eq!(f, 96_000);
    }

    #[test]
    fn resolve_trigger_next_beat_boundary_mid_beat_rounds_up() {
        let f = resolve_trigger(ScheduleTrigger::NextBeatBoundary, t120(), 1, 0, SAMPLE_RATE);
        // beat at 120 BPM is 24000 frames
        assert_eq!(f, 24_000);
    }

    #[test]
    fn lead_time_guard_uses_resolved_frame_for_planned_origins() {
        // simulate a PlannedLlm scheduling via NextBarBoundary at
        // engine_now = 0: next bar = 96_000 frames = 2 seconds, well
        // under the 30s lead-time. Resolved frame is 96_000 → fails
        // when fed into validate_schedule_request.
        let resolved = resolve_trigger(ScheduleTrigger::NextBarBoundary, t120(), 0, 0, SAMPLE_RATE);
        let err = validate_schedule_request(
            ScheduledActionId::new("next-bar-too-soon"),
            ActionOrigin::PlannedLlm,
            ScheduleRequestTiming {
                submitted_at_frame: 0,
                trigger_frame: resolved,
            },
            SAMPLE_RATE,
        )
        .expect_err("next-bar trigger inside lead-time must be rejected");
        assert!(matches!(
            err,
            ScheduleValidationError::PlannedActionTooSoon { .. }
        ));
    }

    #[test]
    fn relative_musical_at_30s_or_more_passes_lead_time_guard() {
        // 30s @ 120 BPM 4-4 = 15 bars = 60 beats
        let resolved = resolve_trigger(
            ScheduleTrigger::RelativeMusical { bars: 15, beats: 0 },
            t120(),
            0,
            0,
            SAMPLE_RATE,
        );
        assert_eq!(resolved, 1_440_000);
        let ok = validate_schedule_request(
            ScheduledActionId::new("relative-ok"),
            ActionOrigin::PlannedPi,
            ScheduleRequestTiming {
                submitted_at_frame: 0,
                trigger_frame: resolved,
            },
            SAMPLE_RATE,
        );
        assert!(ok.is_ok());
    }
}
