# ADR-002: SynthVoice Trait for Sequencer Backends

**Status**: Accepted

## Context

Phase 2 of the LLM-autonomous-composition roadmap (Epic #1) introduces a
`SequencerSource` audio source that consumes `NoteEvent` batches and
produces audio by triggering polyphonic synthesizer voices. The sequencer
(#6) and the actual sound-generating voices (#7) are distinct concerns:

| Component | Owns |
|-----------|------|
| `SequencerSource` (#6) | scheduling, voice allocation, voice stealing, mixing |
| `SynthVoice` impl (#7) | oscillator state, envelope state, per-sample generation |

For these two pieces to evolve independently — and to allow later voice
backends (SoundFont/SF2, FM, wavetable, Glicol-as-voice) without changing
the sequencer — they must communicate through a stable abstraction.

This ADR fixes that abstraction so #6 and #7 can be implemented in
parallel without churn during Wave 1 / Wave 4 of the roadmap.

## Decision

Introduce a `SynthVoice` trait with the following signature in
`crates/omm-audio/src/source/synth.rs`:

```rust
pub trait SynthVoice: Send {
    /// Begin a new note with the given MIDI pitch (0..=127) and velocity
    /// (0..=127). Resets the envelope; replaces any currently sounding
    /// note on this voice instance.
    fn note_on(&mut self, pitch: u8, velocity: u8);

    /// Release the note matching `pitch`. If the voice is currently
    /// sounding a different pitch, this is a no-op. The voice transitions
    /// into the release phase of its envelope and remains active until
    /// release completes.
    fn note_off(&mut self, pitch: u8);

    /// Render `out.len()` stereo frames of audio. The voice OVERWRITES
    /// `out` rather than summing — the caller is responsible for mixing
    /// multiple voices. An idle voice writes silence.
    fn render(&mut self, out: &mut [StereoFrame]);

    /// True when the voice is currently producing or could produce
    /// non-silent output (envelope is not in the Idle state).
    fn is_active(&self) -> bool;

    /// The pitch currently held by this voice, if any. Used by the
    /// sequencer for voice stealing.
    fn current_pitch(&self) -> Option<u8>;
}
```

### Specifics

1. **`Send` bound, no `Sync`.** Voices live inside `SequencerSource`,
   which is owned by `ChannelStrip`. Voices are constructed off the audio
   thread and moved into the runtime; no concurrent access is required.
   `Sync` would over-constrain implementations (e.g. ones with internal
   `Cell`/`RefCell`).

2. **`render` overwrites, does not sum.** Removes the "did the caller
   zero the buffer?" ambiguity. The sequencer renders each voice into a
   scratch buffer, then sums voice scratches into the channel output.
   Cost is one extra stereo buffer copy per voice per block — negligible
   at 16 voices × 256 frames × 2 channels × `f32` = 32 KiB/block at
   48 kHz.

3. **`current_pitch` returns `Option<u8>`.** `None` when the voice is
   idle. Lets the sequencer pick stealing victims without inspecting
   internal voice state. Voice stealing policy lives entirely in the
   sequencer (see #6 issue body). The sequencer tracks per-voice note
   age externally; the trait does not expose it, to keep the surface
   minimal.

4. **No per-voice gain, pan, or filter parameters.** Those belong to the
   `ChannelStrip` that the voice's output flows into. Keeping the voice
   purely about pitch / envelope / timbre matches the existing per-source
   effect topology described in `docs/ARCHITECTURE.md` §6.1.

5. **Velocity normalization** is the voice's responsibility. Standard
   mapping is `velocity / 127.0` linearly applied to envelope amplitude.
   Non-linear curves are voice-specific.

6. **Mono → stereo replication** is the voice's responsibility. A mono
   oscillator writes the same sample to both `left` and `right`. Stereo
   placement (pan, width) is the `ChannelStrip`'s job.

7. **RT safety is a hard contract on implementers.** `render` is invoked
   from the audio callback and MUST NOT allocate, lock, log, await, or
   take any other action forbidden by `docs/ARCHITECTURE.md` §5.2.
   `note_on` and `note_off` are also called from the audio callback
   (driven by the sequencer's due-note pump) and share the same
   constraints. Constructors run off the audio thread and may allocate.
   Constructors receive `sample_rate` and store it; the trait
   intentionally excludes `sample_rate` from `render`.

## Consequences

- #6 (`SequencerSource`) and #7 (mini-synth backend) can be implemented
  in parallel during Wave 1 / Wave 4 against this trait without
  coordination.
- Future voice backends (SoundFont/SF2, FM, wavetable, Glicol-as-voice)
  implement the same trait and drop into existing sequencers unchanged.
- Voice stealing strategy is centralized in the sequencer; voices do not
  cooperate.
- Per-voice filter / per-voice modulation are intentionally deferred.
  When they are added (Phase 3+), they may join the trait via a default-
  implementation extension method or a separate `SynthVoiceFx` trait to
  avoid breaking existing implementations.
- The `voice → scratch → sequencer mix → ChannelStrip` data path costs
  one extra stereo buffer copy per voice per block. Acceptable given the
  unambiguous overwrite contract it buys.

Refs: Epic #1, Issue #6 (Phase 2b SequencerSource), Issue #7
(Phase 2c Mini-synth).
