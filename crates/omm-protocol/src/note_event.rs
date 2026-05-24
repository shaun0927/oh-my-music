use serde::{Deserialize, Serialize};

use crate::musical_time::MusicalTime;
use crate::source_timeline::SourceInstanceId;

/// A single MIDI-style note event scheduled at a musical position.
///
/// Lifetime model: a single `NoteEvent` carries its own length
/// (`length_ticks`) — separate note-off events are NOT used. The
/// sequencer that consumes the event derives the note-off moment from
/// `start + length_ticks` under the current `Transport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoteEvent {
    pub pitch_midi: u8,
    pub velocity: u8,
    pub start: MusicalTime,
    pub length_ticks: u32,
    pub channel: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoteEventValidationError {
    #[error("pitch_midi must be in 0..=127, got {0}")]
    PitchOutOfRange(u8),
    #[error("velocity must be in 0..=127, got {0}")]
    VelocityOutOfRange(u8),
    #[error("length_ticks must be > 0")]
    ZeroLength,
    #[error("events must be sorted by start (offending index: {0})")]
    UnsortedEvents(usize),
}

impl NoteEvent {
    pub fn new(
        pitch_midi: u8,
        velocity: u8,
        start: MusicalTime,
        length_ticks: u32,
        channel: u8,
    ) -> Self {
        Self {
            pitch_midi,
            velocity,
            start,
            length_ticks,
            channel,
        }
    }

    pub fn validate(&self) -> Result<(), NoteEventValidationError> {
        if self.pitch_midi > 127 {
            return Err(NoteEventValidationError::PitchOutOfRange(self.pitch_midi));
        }
        if self.velocity > 127 {
            return Err(NoteEventValidationError::VelocityOutOfRange(self.velocity));
        }
        if self.length_ticks == 0 {
            return Err(NoteEventValidationError::ZeroLength);
        }
        Ok(())
    }
}

/// A batch of `NoteEvent`s targeting one source instance. Optional
/// loop point: when `loop_bars = Some(n)`, the batch repeats every
/// `n` bars (under the current `Transport`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteEventBatch {
    pub source_instance_id: SourceInstanceId,
    pub events: Vec<NoteEvent>,
    pub loop_bars: Option<u32>,
}

impl NoteEventBatch {
    pub fn new(source_instance_id: SourceInstanceId, events: Vec<NoteEvent>) -> Self {
        Self {
            source_instance_id,
            events,
            loop_bars: None,
        }
    }

    pub fn with_loop_bars(mut self, bars: u32) -> Self {
        self.loop_bars = Some(bars);
        self
    }

    /// Validate every event AND that the events are sorted by their
    /// `start` musical position. Returns the index of the first
    /// offending event on failure.
    pub fn validate(&self) -> Result<(), NoteEventValidationError> {
        let mut prev: Option<MusicalTime> = None;
        for (index, event) in self.events.iter().enumerate() {
            event.validate()?;
            if let Some(p) = prev {
                if event.start < p {
                    return Err(NoteEventValidationError::UnsortedEvents(index));
                }
            }
            prev = Some(event.start);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ne(pitch: u8, bar: u32, beat: u16, length: u32) -> NoteEvent {
        NoteEvent::new(pitch, 100, MusicalTime::new(bar, beat, 0), length, 0)
    }

    #[test]
    fn validation_rejects_out_of_range_pitch() {
        let mut e = ne(60, 0, 0, 480);
        e.pitch_midi = 128;
        assert!(matches!(
            e.validate(),
            Err(NoteEventValidationError::PitchOutOfRange(128))
        ));
    }

    #[test]
    fn validation_rejects_out_of_range_velocity() {
        let mut e = ne(60, 0, 0, 480);
        e.velocity = 200;
        assert!(matches!(
            e.validate(),
            Err(NoteEventValidationError::VelocityOutOfRange(200))
        ));
    }

    #[test]
    fn validation_rejects_zero_length() {
        let mut e = ne(60, 0, 0, 480);
        e.length_ticks = 0;
        assert!(matches!(
            e.validate(),
            Err(NoteEventValidationError::ZeroLength)
        ));
    }

    #[test]
    fn validation_accepts_well_formed_event() {
        assert!(ne(60, 0, 0, 480).validate().is_ok());
        assert!(ne(0, 0, 0, 1).validate().is_ok());
        assert!(ne(127, 0, 0, 1).validate().is_ok());
    }

    #[test]
    fn batch_validation_rejects_unsorted_events() {
        let id = SourceInstanceId::new("seq:test");
        let batch = NoteEventBatch::new(
            id,
            vec![
                ne(60, 0, 2, 480),
                ne(64, 0, 1, 480), // out of order
            ],
        );
        assert!(matches!(
            batch.validate(),
            Err(NoteEventValidationError::UnsortedEvents(1))
        ));
    }

    #[test]
    fn batch_validation_accepts_sorted_events_with_loop() {
        let id = SourceInstanceId::new("seq:test");
        let batch = NoteEventBatch::new(
            id,
            vec![
                ne(60, 0, 0, 480),
                ne(62, 0, 1, 480),
                ne(64, 0, 2, 480),
                ne(67, 0, 3, 480),
            ],
        )
        .with_loop_bars(1);
        assert!(batch.validate().is_ok());
    }

    #[test]
    fn batch_serializes_round_trip_with_thousand_events() {
        let id = SourceInstanceId::new("seq:big");
        let events: Vec<NoteEvent> = (0..1000)
            .map(|i| ne(60 + (i as u8 % 24), i / 4, (i % 4) as u16, 240))
            .collect();
        let batch = NoteEventBatch::new(id, events);
        assert!(batch.validate().is_ok());
        let json = serde_json::to_string(&batch).unwrap();
        let back: NoteEventBatch = serde_json::from_str(&json).unwrap();
        assert_eq!(back, batch);
    }

    #[test]
    fn note_event_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<NoteEvent>();
    }
}
