//! Lock-free SPSC queue for `NoteEvent`s flowing from the control side
//! into a `SequencerSource` running on the audio thread. Mirrors the
//! pattern in `crate::command` so RT-safety guarantees are identical.

use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

use omm_protocol::NoteEvent;

/// Maximum in-flight `NoteEvent`s per `SequencerSource`. Chosen so a
/// 1000-event batch (the IPC batch ceiling) can land in a single push
/// burst with headroom.
pub const RT_NOTE_QUEUE_CAPACITY: usize = 4096;

/// Maximum number of events the sequencer drains in a single render
/// callback. Sized so a 128-frame block at 120 BPM cannot exhaust the
/// queue even with dense polyphony.
pub const MAX_NOTE_DRAIN_PER_BLOCK: usize = 256;

pub struct NoteEventQueue {
    producer: HeapProd<NoteEvent>,
}

pub struct NoteEventReceiver {
    consumer: HeapCons<NoteEvent>,
}

#[derive(Debug, thiserror::Error)]
#[error("RT note event queue full")]
pub struct NoteQueueFull;

pub fn new_note_channel() -> (NoteEventQueue, NoteEventReceiver) {
    let rb = HeapRb::<NoteEvent>::new(RT_NOTE_QUEUE_CAPACITY);
    let (producer, consumer) = rb.split();
    (NoteEventQueue { producer }, NoteEventReceiver { consumer })
}

impl NoteEventQueue {
    /// Push a single event. Returns `NoteQueueFull` if the consumer
    /// has not caught up.
    pub fn enqueue(&mut self, event: NoteEvent) -> Result<(), NoteQueueFull> {
        self.producer.try_push(event).map_err(|_| NoteQueueFull)
    }

    /// Push every event from `batch_events` until the queue fills.
    /// Returns the number of events successfully enqueued. Caller can
    /// retry with the remaining slice.
    pub fn enqueue_many(&mut self, batch_events: &[NoteEvent]) -> usize {
        let mut count = 0;
        for event in batch_events {
            if self.producer.try_push(*event).is_err() {
                break;
            }
            count += 1;
        }
        count
    }

    pub fn capacity(&self) -> usize {
        RT_NOTE_QUEUE_CAPACITY
    }
}

impl NoteEventReceiver {
    /// Drain up to `max` events into `sink`. Returns the count drained.
    pub fn drain(&mut self, sink: &mut impl FnMut(NoteEvent), max: usize) -> usize {
        let mut count = 0;
        while count < max {
            let Some(event) = self.consumer.try_pop() else {
                break;
            };
            sink(event);
            count += 1;
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omm_protocol::MusicalTime;
    use std::thread;

    fn note(index: u32) -> NoteEvent {
        NoteEvent::new(
            60 + (index as u8 % 24),
            100,
            MusicalTime::new(index / 4, (index % 4) as u16, 0),
            240,
            0,
        )
    }

    fn note_index(event: NoteEvent) -> u32 {
        event.start.bar * 4 + event.start.beat as u32
    }

    fn assert_send<T: Send>() {}

    #[test]
    fn queue_and_receiver_are_send() {
        assert_send::<NoteEventQueue>();
        assert_send::<NoteEventReceiver>();
    }

    #[test]
    fn full_capacity_accepts_capacity_and_rejects_overflow() {
        let (mut queue, _rx) = new_note_channel();
        for i in 0..RT_NOTE_QUEUE_CAPACITY {
            assert!(queue.enqueue(note(i as u32)).is_ok());
        }
        assert!(matches!(
            queue.enqueue(note(RT_NOTE_QUEUE_CAPACITY as u32)),
            Err(NoteQueueFull)
        ));
    }

    #[test]
    fn enqueue_many_stops_at_full_and_reports_count() {
        let (mut queue, _rx) = new_note_channel();
        let events: Vec<NoteEvent> = (0..RT_NOTE_QUEUE_CAPACITY + 100)
            .map(|i| note(i as u32))
            .collect();
        let pushed = queue.enqueue_many(&events);
        assert_eq!(pushed, RT_NOTE_QUEUE_CAPACITY);
    }

    #[test]
    fn drain_returns_events_in_fifo_order() {
        let (mut queue, mut rx) = new_note_channel();
        for i in 0..10 {
            queue.enqueue(note(i)).unwrap();
        }
        let mut drained = Vec::new();
        let count = rx.drain(
            &mut |e| drained.push(note_index(e)),
            MAX_NOTE_DRAIN_PER_BLOCK,
        );
        assert_eq!(count, 10);
        assert_eq!(drained, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn drain_respects_max() {
        let (mut queue, mut rx) = new_note_channel();
        for i in 0..50 {
            queue.enqueue(note(i)).unwrap();
        }
        let mut drained = Vec::new();
        let count = rx.drain(&mut |e| drained.push(note_index(e)), 10);
        assert_eq!(count, 10);
        assert_eq!(drained.len(), 10);
    }

    #[test]
    fn concurrent_spsc_transfers_every_event_once() {
        let (mut queue, mut rx) = new_note_channel();
        let total = 10_000_u32;

        let producer = thread::spawn(move || {
            for i in 0..total {
                while queue.enqueue(note(i)).is_err() {
                    thread::yield_now();
                }
            }
        });

        let consumer = thread::spawn(move || {
            let mut drained = Vec::with_capacity(total as usize);
            while drained.len() < total as usize {
                let n = rx.drain(
                    &mut |e| drained.push(note_index(e)),
                    MAX_NOTE_DRAIN_PER_BLOCK,
                );
                if n == 0 {
                    thread::yield_now();
                }
            }
            drained
        });

        producer.join().unwrap();
        let drained = consumer.join().unwrap();
        assert_eq!(drained.len(), total as usize);
        // FIFO order preserved (single producer single consumer).
        for (i, v) in drained.into_iter().enumerate() {
            assert_eq!(v as usize, i % (total as usize));
        }
    }
}
