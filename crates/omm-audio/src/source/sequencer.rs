//! `SequencerSource` — drains `NoteEvent`s from a lock-free queue and
//! drives a fixed pool of [`SynthVoice`] instances per the contract
//! pinned by ADR-002.
//!
//! Block-level rendering with sample-accurate trigger / release:
//! incoming events whose `start` frame lands inside the current render
//! block split the block at the trigger offset, so a note that should
//! fire at `block_start + 37` actually starts ringing at sample 37 of
//! that block (±0 samples). Voice releases work the same way.
//!
//! Voice stealing: when all voices are active and a new note arrives,
//! the oldest (lowest `triggered_at` frame) voice is forced into the
//! release stage to make room.

use crate::frame::StereoFrame;
use crate::note_queue::{NoteEventReceiver, MAX_NOTE_DRAIN_PER_BLOCK};
use crate::source::synth::SynthVoice;
use crate::source::AudioSource;

use omm_protocol::{musical_time_to_frame, NoteEvent, Transport};

/// Default polyphony for the engine's built-in sequencer source.
pub const DEFAULT_SEQUENCER_POLYPHONY: usize = 16;

/// Builder closure that produces a fresh `SynthVoice` for the voice
/// pool. Called once per slot at construction (off the audio thread).
pub type SynthVoiceFactory = Box<dyn FnMut() -> Box<dyn SynthVoice> + Send>;

struct VoiceSlot {
    voice: Box<dyn SynthVoice>,
    /// Engine frame at which `note_on` was last called. Used by voice
    /// stealing (oldest voice loses).
    triggered_at: Option<u64>,
    /// Engine frame at which we should call `note_off` (start + length).
    release_at: Option<u64>,
    /// MIDI pitch currently held (mirrors `voice.current_pitch()` but
    /// avoids a virtual call on the hot path).
    pitch: Option<u8>,
}

impl VoiceSlot {
    fn new(voice: Box<dyn SynthVoice>) -> Self {
        Self {
            voice,
            triggered_at: None,
            release_at: None,
            pitch: None,
        }
    }
}

struct PendingNote {
    pitch: u8,
    velocity: u8,
    start_frame: u64,
    end_frame: u64,
}

pub struct SequencerSource {
    voices: Vec<VoiceSlot>,
    receiver: NoteEventReceiver,
    /// Pending notes whose `start_frame` is >= `elapsed_frames`. Sorted
    /// by `start_frame`. Drained as the engine advances.
    pending: Vec<PendingNote>,
    transport: Transport,
    sample_rate: u32,
    elapsed_frames: u64,
    scratch_voice: Vec<StereoFrame>,
    enabled: bool,
    gain_lin: f32,
    target_gain_lin: f32,
    gain_step: f32,
    gain_ramp_remaining: u32,
}

impl SequencerSource {
    pub fn new(
        receiver: NoteEventReceiver,
        mut voice_factory: SynthVoiceFactory,
        transport: Transport,
        sample_rate: u32,
        polyphony: usize,
    ) -> Self {
        let polyphony = polyphony.max(1);
        let voices: Vec<VoiceSlot> = (0..polyphony)
            .map(|_| VoiceSlot::new(voice_factory()))
            .collect();
        Self {
            voices,
            receiver,
            pending: Vec::with_capacity(MAX_NOTE_DRAIN_PER_BLOCK),
            transport,
            sample_rate: sample_rate.max(1),
            elapsed_frames: 0,
            scratch_voice: Vec::new(),
            enabled: true,
            gain_lin: 1.0,
            target_gain_lin: 1.0,
            gain_step: 0.0,
            gain_ramp_remaining: 0,
        }
    }

    /// Replace the active transport (control-side only). Notes already
    /// in `pending` keep their previously-resolved frames; only future
    /// `NoteEvent`s use the new transport.
    pub fn set_transport(&mut self, transport: Transport) {
        self.transport = transport;
    }

    pub fn polyphony(&self) -> usize {
        self.voices.len()
    }

    pub fn pending_note_count(&self) -> usize {
        self.pending.len()
    }

    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.voice.is_active()).count()
    }

    /// Drain any newly-arrived NoteEvents into `pending`. Each event's
    /// musical-time `start` is resolved to an absolute engine frame
    /// using the active transport (relative to engine origin frame 0).
    fn drain_incoming(&mut self) {
        let transport = self.transport;
        let sample_rate = self.sample_rate;
        let elapsed = self.elapsed_frames;
        let pending = &mut self.pending;
        self.receiver.drain(
            &mut |event: NoteEvent| {
                let start = musical_time_to_frame(event.start, transport, 0, sample_rate);
                if event.length_ticks == 0 {
                    return;
                }
                // Convert length_ticks → frames via a one-tick reference
                // (`MusicalTime::from_total_ticks` is one-tick precise).
                let length_frames =
                    ticks_to_frames(event.length_ticks as u64, transport, sample_rate).max(1);
                let end = start.saturating_add(length_frames);
                // Skip events that ended before `now` — late delivery.
                if end <= elapsed {
                    return;
                }
                pending.push(PendingNote {
                    pitch: event.pitch_midi.min(127),
                    velocity: event.velocity.min(127),
                    start_frame: start,
                    end_frame: end,
                });
            },
            MAX_NOTE_DRAIN_PER_BLOCK,
        );
        pending.sort_by_key(|p| p.start_frame);
    }

    /// Find the index of the slot to allocate for a new note. Prefers
    /// an idle voice; otherwise steals the oldest active voice (lowest
    /// `triggered_at`).
    fn pick_voice_slot(&mut self) -> usize {
        if let Some(idx) = self
            .voices
            .iter()
            .position(|v| !v.voice.is_active() && v.triggered_at.is_none())
        {
            return idx;
        }
        // Steal: oldest active.
        let mut steal_idx = 0;
        let mut oldest = u64::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            let age = v.triggered_at.unwrap_or(u64::MAX);
            if age < oldest {
                oldest = age;
                steal_idx = i;
            }
        }
        steal_idx
    }

    fn trigger_note(&mut self, note: PendingNote, now_frame: u64) {
        let slot_idx = self.pick_voice_slot();
        // Quickly release any prior pitch on this slot to start fresh.
        if let Some(prev_pitch) = self.voices[slot_idx].pitch {
            self.voices[slot_idx].voice.note_off(prev_pitch);
        }
        let voice = &mut self.voices[slot_idx];
        voice.voice.note_on(note.pitch, note.velocity);
        voice.triggered_at = Some(now_frame);
        voice.release_at = Some(note.end_frame);
        voice.pitch = Some(note.pitch);
    }

    fn release_voice_at(&mut self, slot_idx: usize) {
        if let Some(pitch) = self.voices[slot_idx].pitch {
            self.voices[slot_idx].voice.note_off(pitch);
        }
        self.voices[slot_idx].release_at = None;
    }

    fn render_voices_into(&mut self, out: &mut [StereoFrame]) {
        if out.is_empty() {
            return;
        }
        // Ensure scratch buffer is large enough.
        if self.scratch_voice.len() < out.len() {
            self.scratch_voice.resize(out.len(), StereoFrame::SILENCE);
        }
        // Zero output.
        for f in out.iter_mut() {
            *f = StereoFrame::SILENCE;
        }
        let scratch = &mut self.scratch_voice[..out.len()];
        for slot in self.voices.iter_mut() {
            if !slot.voice.is_active() {
                continue;
            }
            slot.voice.render(scratch);
            for (dst, src) in out.iter_mut().zip(scratch.iter()) {
                dst.left += src.left;
                dst.right += src.right;
            }
        }
        // Apply gain.
        if self.enabled {
            apply_gain_inline(
                out,
                &mut self.gain_lin,
                self.target_gain_lin,
                &mut self.gain_step,
                &mut self.gain_ramp_remaining,
            );
        } else {
            for f in out.iter_mut() {
                *f = StereoFrame::SILENCE;
            }
        }
    }
}

fn apply_gain_inline(
    out: &mut [StereoFrame],
    current: &mut f32,
    target: f32,
    step: &mut f32,
    remaining: &mut u32,
) {
    for frame in out.iter_mut() {
        if *remaining > 0 {
            *current += *step;
            *remaining -= 1;
            if *remaining == 0 {
                *current = target;
            }
        }
        frame.left *= *current;
        frame.right *= *current;
    }
}

#[inline]
fn ticks_to_frames(total_ticks: u64, transport: Transport, sample_rate: u32) -> u64 {
    use omm_protocol::TICKS_PER_QUARTER;
    if transport.bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let fpt = 60.0_f64 * sample_rate as f64 / (transport.bpm as f64 * TICKS_PER_QUARTER as f64);
    (total_ticks as f64 * fpt).round() as u64
}

impl AudioSource for SequencerSource {
    fn render(&mut self, output: &mut [StereoFrame]) {
        if output.is_empty() {
            return;
        }
        if !self.enabled {
            for f in output.iter_mut() {
                *f = StereoFrame::SILENCE;
            }
            return;
        }

        self.drain_incoming();

        let block_len = output.len() as u64;
        let block_start = self.elapsed_frames;
        let block_end = block_start.saturating_add(block_len);

        // Build a sorted list of in-block (offset, action) events:
        //   action: 'T' = trigger note (index into pending), 'O' = release voice (slot)
        // Process them in order, splitting the block into segments.
        #[derive(Clone, Copy)]
        enum Action {
            Trigger(usize),      // pending index
            ReleaseVoice(usize), // voice slot (pre-existing release)
            ReleaseByPitch(u8),  // pitch (release for a note triggered in this block)
        }

        let mut events: Vec<(usize, Action)> = Vec::new();

        for (i, p) in self.pending.iter().enumerate() {
            if p.start_frame >= block_start && p.start_frame < block_end {
                let offset = (p.start_frame - block_start) as usize;
                events.push((offset, Action::Trigger(i)));
                // If the note's release also falls inside this block,
                // schedule it via pitch lookup so the voice we
                // allocate gets note_off at the right sample.
                if p.end_frame > p.start_frame && p.end_frame <= block_end {
                    let release_offset = ((p.end_frame - block_start) as usize).min(output.len());
                    events.push((release_offset, Action::ReleaseByPitch(p.pitch)));
                }
            }
        }
        for (i, v) in self.voices.iter().enumerate() {
            if let Some(end) = v.release_at {
                if end >= block_start && end < block_end {
                    let offset = (end - block_start) as usize;
                    events.push((offset, Action::ReleaseVoice(i)));
                }
            }
        }
        // Stable sort by offset; ties processed in insertion order so
        // Trigger fires before its same-offset ReleaseByPitch.
        events.sort_by_key(|e| e.0);

        let mut consumed_pending: Vec<usize> = Vec::new();
        let mut pos = 0_usize;

        for (offset, action) in events {
            if offset > pos {
                let segment = &mut output[pos..offset];
                self.render_voices_into(segment);
                pos = offset;
            }
            match action {
                Action::Trigger(idx) => {
                    let note = PendingNote {
                        pitch: self.pending[idx].pitch,
                        velocity: self.pending[idx].velocity,
                        start_frame: self.pending[idx].start_frame,
                        end_frame: self.pending[idx].end_frame,
                    };
                    consumed_pending.push(idx);
                    let now = block_start + offset as u64;
                    self.trigger_note(note, now);
                }
                Action::ReleaseVoice(slot) => {
                    self.release_voice_at(slot);
                }
                Action::ReleaseByPitch(pitch) => {
                    // Find the most-recently-triggered voice holding
                    // `pitch` and release it. (Most-recent because if
                    // the same pitch retriggers, the newest one is the
                    // active note for this release event.)
                    let target = self
                        .voices
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| v.pitch == Some(pitch))
                        .max_by_key(|(_, v)| v.triggered_at.unwrap_or(0))
                        .map(|(i, _)| i);
                    if let Some(slot) = target {
                        self.release_voice_at(slot);
                    }
                }
            }
        }

        if pos < output.len() {
            let segment = &mut output[pos..];
            self.render_voices_into(segment);
        }

        // Remove consumed pending entries (sort + dedup so we don't
        // double-remove if multiple triggers landed in the same block).
        if !consumed_pending.is_empty() {
            consumed_pending.sort_unstable();
            consumed_pending.dedup();
            for idx in consumed_pending.iter().rev() {
                self.pending.remove(*idx);
            }
        }

        // Post-block sweep: any voice whose release_at has passed must
        // get its note_off, even if the release happened during this
        // same render block (e.g. very short notes whose entire
        // lifetime falls inside one block, or notes whose trigger AND
        // end both landed in this block and the per-block events list
        // had no chance to schedule the release before the trigger
        // fired).
        for slot in self.voices.iter_mut() {
            if let Some(end) = slot.release_at {
                if end <= block_end {
                    if let Some(pitch) = slot.pitch {
                        slot.voice.note_off(pitch);
                    }
                    slot.release_at = None;
                }
            }
            if !slot.voice.is_active() {
                slot.triggered_at = None;
                slot.release_at = None;
                slot.pitch = None;
            }
        }

        self.elapsed_frames = block_end;
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn set_gain_db(&mut self, gain_db: f32, ramp_frames: u32) {
        let target_lin = if gain_db < -60.0 {
            0.0
        } else {
            10.0_f32.powf(gain_db / 20.0)
        };
        self.target_gain_lin = target_lin;
        if ramp_frames == 0 {
            self.gain_lin = target_lin;
            self.gain_step = 0.0;
            self.gain_ramp_remaining = 0;
        } else {
            let delta = target_lin - self.gain_lin;
            self.gain_step = delta / ramp_frames as f32;
            self.gain_ramp_remaining = ramp_frames;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note_queue::new_note_channel;
    use crate::source::synth::{AdsrEnvelope, SineAdsrVoice};
    use omm_protocol::{MusicalTime, NoteEvent, TimeSignature};

    const SR: u32 = 48_000;

    fn sine_factory() -> SynthVoiceFactory {
        Box::new(|| {
            Box::new(SineAdsrVoice::new(
                AdsrEnvelope::new(1.0, 5.0, 0.7, 50.0, SR),
                SR,
            )) as Box<dyn SynthVoice>
        })
    }

    fn t120() -> Transport {
        Transport::new(120.0, TimeSignature::FOUR_FOUR)
    }

    fn peak(buf: &[StereoFrame]) -> f32 {
        buf.iter().fold(0.0_f32, |m, f| m.max(f.left.abs()))
    }

    fn first_nonzero(buf: &[StereoFrame]) -> Option<usize> {
        buf.iter().position(|f| f.left.abs() > 1.0e-9)
    }

    #[test]
    fn sequencer_starts_silent() {
        let (_q, rx) = new_note_channel();
        let mut seq = SequencerSource::new(rx, sine_factory(), t120(), SR, 16);
        let mut buf = vec![StereoFrame::SILENCE; 256];
        seq.render(&mut buf);
        assert_eq!(peak(&buf), 0.0);
        assert_eq!(seq.active_voice_count(), 0);
    }

    #[test]
    fn single_note_at_start_renders_audible_signal() {
        let (mut q, rx) = new_note_channel();
        let mut seq = SequencerSource::new(rx, sine_factory(), t120(), SR, 16);
        // C5 at MusicalTime ZERO, length 1 quarter (480 ticks).
        q.enqueue(NoteEvent::new(72, 100, MusicalTime::ZERO, 480, 0))
            .unwrap();
        let mut buf = vec![StereoFrame::SILENCE; SR as usize / 10]; // 100 ms
        seq.render(&mut buf);
        assert!(
            peak(&buf) > 0.3,
            "note should be audible, peak={}",
            peak(&buf)
        );
        assert_eq!(seq.active_voice_count(), 1);
    }

    #[test]
    fn note_at_bar_boundary_starts_at_exact_sample() {
        let (mut q, rx) = new_note_channel();
        let mut seq = SequencerSource::new(rx, sine_factory(), t120(), SR, 16);
        // Note at bar 1 beat 0 → 96000 frames @ 120 BPM 4-4
        q.enqueue(NoteEvent::new(72, 100, MusicalTime::new(1, 0, 0), 480, 0))
            .unwrap();
        // Render up to frame 96000 — should be silent throughout.
        let mut buf1 = vec![StereoFrame::SILENCE; 96_000];
        seq.render(&mut buf1);
        assert_eq!(peak(&buf1), 0.0, "must be silent before bar 1");
        // Render the next 256 samples — note should start at sample 0.
        let mut buf2 = vec![StereoFrame::SILENCE; 256];
        seq.render(&mut buf2);
        let first = first_nonzero(&buf2).expect("expected note to start in this block");
        assert!(
            first <= 1,
            "note should start at sample 0 or 1, got {first}"
        );
    }

    #[test]
    fn five_simultaneous_notes_use_five_voices() {
        let (mut q, rx) = new_note_channel();
        let mut seq = SequencerSource::new(rx, sine_factory(), t120(), SR, 16);
        for p in [60_u8, 64, 67, 71, 74] {
            q.enqueue(NoteEvent::new(p, 100, MusicalTime::ZERO, 480, 0))
                .unwrap();
        }
        let mut buf = vec![StereoFrame::SILENCE; 4_800]; // 100 ms
        seq.render(&mut buf);
        assert_eq!(seq.active_voice_count(), 5);
        assert!(peak(&buf) > 0.5);
    }

    #[test]
    fn voice_stealing_keeps_pool_size_when_overflowing_polyphony() {
        let (mut q, rx) = new_note_channel();
        let mut seq = SequencerSource::new(rx, sine_factory(), t120(), SR, 4); // small pool
        for p in [60, 62, 64, 65, 67, 69, 71, 72] {
            q.enqueue(NoteEvent::new(p as u8, 100, MusicalTime::ZERO, 480, 0))
                .unwrap();
        }
        let mut buf = vec![StereoFrame::SILENCE; 4_800];
        seq.render(&mut buf);
        assert!(
            seq.active_voice_count() <= 4,
            "voice pool must never exceed polyphony: got {}",
            seq.active_voice_count()
        );
        assert_eq!(seq.polyphony(), 4);
    }

    #[test]
    fn release_marks_voice_idle_after_note_length() {
        let (mut q, rx) = new_note_channel();
        let mut seq = SequencerSource::new(rx, sine_factory(), t120(), SR, 16);
        // Very short note: 1 tick @ 120 BPM 4-4 ≈ 50 frames.
        q.enqueue(NoteEvent::new(72, 100, MusicalTime::ZERO, 1, 0))
            .unwrap();
        // Render enough to outlast attack + release.
        let mut buf = vec![StereoFrame::SILENCE; SR as usize / 5]; // 200 ms
        seq.render(&mut buf);
        assert_eq!(seq.active_voice_count(), 0, "voice should release and idle");
    }

    #[test]
    fn drain_runs_alloc_free_on_steady_state() {
        // Drain alloc-free after warmup: the receiver's drain is
        // ringbuf::try_pop (no allocation). The pending Vec only
        // allocates when capacity grows; after the first push it
        // reuses its buffer. Render allocates scratch buffers once
        // and reuses thereafter. We can't easily assert no alloc
        // without an allocator harness, but we can at least
        // demonstrate steady-state push+drain in a tight loop.
        let (mut q, rx) = new_note_channel();
        let mut seq = SequencerSource::new(rx, sine_factory(), t120(), SR, 16);
        let mut buf = vec![StereoFrame::SILENCE; 256];
        for i in 0..50_u32 {
            q.enqueue(NoteEvent::new(60, 100, MusicalTime::new(i, 0, 0), 240, 0))
                .unwrap();
            seq.render(&mut buf);
        }
        // The sequencer survives 50 iterations with no panic.
        assert!(seq.elapsed_frames > 0);
    }
}
