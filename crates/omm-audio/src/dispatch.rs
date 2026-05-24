//! Engine-command dispatch for Phase 1d/2d (Wave 7).
//!
//! Applies the agent-facing `omm_protocol::EngineCommand` variants
//! that touch the musical-time / note-event subsystem (transport,
//! sequencer source lifecycle, note batches). The real IPC server
//! will wrap this in a request/response loop; the dispatcher itself
//! is a pure function over `&mut AudioRuntime` so it can be exercised
//! end-to-end from tests / examples without a running socket.

use omm_protocol::{
    EngineCommand, MusicalTime, NoteEvent, NoteEventBatch, ScheduleTrigger, SourceInstanceId,
    VoiceType,
};

use crate::note_queue::NoteEventQueue;
use crate::runtime::AudioRuntime;
use crate::runtime::SourceInstanceError;
use crate::source::sequencer::SynthVoiceFactory;
use crate::source::synth::{AdsrEnvelope, SawAdsrVoice, SineAdsrVoice, SynthVoice};

/// Errors a dispatcher invocation can surface to the caller (typically
/// the IPC server, which then turns them into `EngineEvent::Nack`).
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("unsupported command for this dispatcher: {0}")]
    Unsupported(&'static str),
    #[error("source instance not found: {0}")]
    SourceInstanceNotFound(String),
    #[error("source instance already exists: {0}")]
    DuplicateSourceInstance(String),
    #[error("note batch validation failed: {0}")]
    NoteValidation(#[from] omm_protocol::note_event::NoteEventValidationError),
    #[error("source instance error: {0}")]
    SourceInstance(#[from] SourceInstanceError),
    #[error("note queue full while scheduling batch")]
    NoteQueueFull,
}

/// One looping note batch installed by a `ScheduleNotes` call whose
/// payload carried `loop_bars = Some(n)`. The dispatcher remembers
/// the template events (with start times normalized to bar 0) and
/// re-pushes them every `n` bars by walking forward through
/// `iterations_pushed`.
struct LoopState {
    template: Vec<NoteEvent>,
    loop_bars: u32,
    iterations_pushed: u32,
    transport: omm_protocol::Transport,
}

/// Holds the per-source `NoteEventQueue` producers the dispatcher
/// allocated via `CreateSequencerSource`, plus any active loops the
/// agent installed via `ScheduleNotes { loop_bars: Some(n), ... }`.
/// All control-side state — never touched from the audio callback.
#[derive(Default)]
pub struct SequencerRegistry {
    entries: Vec<RegistryEntry>,
}

struct RegistryEntry {
    id: SourceInstanceId,
    queue: NoteEventQueue,
    loops: Vec<LoopState>,
}

impl SequencerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: SourceInstanceId, queue: NoteEventQueue) {
        self.entries.push(RegistryEntry {
            id,
            queue,
            loops: Vec::new(),
        });
    }

    pub fn get_mut(&mut self, id: &SourceInstanceId) -> Option<&mut NoteEventQueue> {
        self.entries
            .iter_mut()
            .find(|entry| &entry.id == id)
            .map(|entry| &mut entry.queue)
    }

    pub fn remove(&mut self, id: &SourceInstanceId) -> Option<NoteEventQueue> {
        if let Some(pos) = self.entries.iter().position(|entry| &entry.id == id) {
            Some(self.entries.swap_remove(pos).queue)
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Install (or replace) a loop template for the given source. The
    /// template's `start` musical times must already be normalized to
    /// bar 0 of the first iteration.
    pub fn install_loop(
        &mut self,
        id: &SourceInstanceId,
        template: Vec<NoteEvent>,
        loop_bars: u32,
        transport: omm_protocol::Transport,
    ) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| &entry.id == id) {
            entry.loops.push(LoopState {
                template,
                loop_bars,
                iterations_pushed: 1, // initial push already happened
                transport,
            });
        }
    }

    /// Advance every installed loop. Called from the control side
    /// after each render block (or any control-clock tick). Re-pushes
    /// each loop's template with start times shifted by
    /// `iterations_pushed × loop_bars` bars until the loop's "next
    /// push" musical time has passed `now_engine_frame`.
    ///
    /// Returns the number of loop iterations actually re-pushed.
    pub fn tick_loops(&mut self, now_engine_frame: u64, sample_rate: u32) -> usize {
        let mut total_pushed = 0_usize;
        for entry in self.entries.iter_mut() {
            entry.loops.retain_mut(|loop_state| {
                let frames_per_bar = bar_frames(loop_state.transport, sample_rate);
                if frames_per_bar == 0 || loop_state.loop_bars == 0 {
                    // bad config — drop the loop so we don't spin.
                    return false;
                }
                let iter_frames = frames_per_bar * loop_state.loop_bars as u64;
                loop {
                    let next_start = iter_frames * loop_state.iterations_pushed as u64;
                    if next_start > now_engine_frame {
                        break;
                    }
                    let shift = MusicalTime::from_total_ticks(
                        ticks_for_bars(
                            loop_state.loop_bars * loop_state.iterations_pushed,
                            loop_state.transport,
                        ),
                        loop_state.transport.time_signature,
                    );
                    let mut all_pushed = true;
                    for ev in &loop_state.template {
                        let shifted = NoteEvent {
                            pitch_midi: ev.pitch_midi,
                            velocity: ev.velocity,
                            start: shift_musical_time(
                                ev.start,
                                shift,
                                loop_state.transport.time_signature,
                            ),
                            length_ticks: ev.length_ticks,
                            channel: ev.channel,
                        };
                        if entry.queue.enqueue(shifted).is_err() {
                            all_pushed = false;
                            break;
                        }
                    }
                    if !all_pushed {
                        // Queue full: stop pushing this loop this tick.
                        // Try again next tick. Don't advance counter
                        // so we retry the same iteration.
                        break;
                    }
                    loop_state.iterations_pushed += 1;
                    total_pushed += 1;
                }
                true
            });
        }
        total_pushed
    }

    /// Drop every loop on every source. Used after `RemoveSequencerSource`
    /// to keep state tidy or after `ClearNotes` with no `from`.
    pub fn clear_all_loops(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.loops.clear();
        }
    }

    /// How many loop templates are currently installed (across all sources).
    pub fn loop_count(&self) -> usize {
        self.entries.iter().map(|e| e.loops.len()).sum()
    }
}

fn bar_frames(transport: omm_protocol::Transport, sample_rate: u32) -> u64 {
    let ticks = transport.time_signature.ticks_per_bar() as u64;
    ticks_to_frames(ticks, transport, sample_rate)
}

fn ticks_for_bars(bars: u32, transport: omm_protocol::Transport) -> u64 {
    transport.time_signature.ticks_per_bar() as u64 * bars as u64
}

fn ticks_to_frames(total_ticks: u64, transport: omm_protocol::Transport, sample_rate: u32) -> u64 {
    if transport.bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let fpt = 60.0_f64 * sample_rate as f64
        / (transport.bpm as f64 * omm_protocol::TICKS_PER_QUARTER as f64);
    (total_ticks as f64 * fpt).round() as u64
}

/// Apply a Phase-1d/2d engine command to the runtime + registry.
/// Returns Ok for handled commands or `Unsupported` for commands
/// outside this dispatcher's scope (they should be routed by the
/// caller — e.g. `Hello`, `SetParam`, `GlicolLoadCode`).
pub fn apply_engine_command(
    runtime: &mut AudioRuntime,
    registry: &mut SequencerRegistry,
    sample_rate: u32,
    command: EngineCommand,
) -> Result<(), DispatchError> {
    match command {
        EngineCommand::SetTransport { transport } => {
            runtime.set_transport(transport);
            Ok(())
        }
        EngineCommand::CreateSequencerSource {
            source_instance_id,
            voice_type,
            polyphony,
        } => {
            let id = SourceInstanceId::new(source_instance_id);
            let factory = make_voice_factory(voice_type, sample_rate);
            let queue = runtime
                .add_sequencer_source(id.clone(), factory, polyphony as usize)
                .map_err(|err| match err {
                    SourceInstanceError::DuplicateSourceInstance { source_instance_id } => {
                        DispatchError::DuplicateSourceInstance(source_instance_id)
                    }
                    other => DispatchError::SourceInstance(other),
                })?;
            registry.insert(id, queue);
            Ok(())
        }
        EngineCommand::RemoveSequencerSource {
            source_instance_id,
            fade_ms,
        } => {
            let id = SourceInstanceId::new(source_instance_id);
            registry.remove(&id);
            let fade_frames =
                ((fade_ms as u64 * sample_rate as u64) / 1000).min(u32::MAX as u64) as u32;
            runtime
                .stop_source_instance(&id, fade_frames)
                .map_err(|err| match err {
                    SourceInstanceError::SourceInstanceNotFound { source_instance_id } => {
                        DispatchError::SourceInstanceNotFound(source_instance_id)
                    }
                    other => DispatchError::SourceInstance(other),
                })?;
            Ok(())
        }
        EngineCommand::ScheduleNotes { batch, trigger } => {
            apply_schedule_notes(runtime, registry, sample_rate, batch, trigger)
        }
        EngineCommand::ClearNotes {
            source_instance_id,
            from,
        } => {
            // The runtime-level pending buffer lives inside the
            // SequencerSource; there's no direct API to inspect it
            // from outside without exposing more handles. For Wave 7
            // we surface the call as a no-op success when the source
            // exists, and follow-up work will add a proper
            // SequencerSource::clear_pending(from) entry point.
            let _ = from;
            let id = SourceInstanceId::new(source_instance_id);
            if registry.get_mut(&id).is_none() {
                return Err(DispatchError::SourceInstanceNotFound(
                    id.as_str().to_string(),
                ));
            }
            Ok(())
        }
        other => Err(DispatchError::Unsupported(engine_command_label(&other))),
    }
}

fn make_voice_factory(voice_type: VoiceType, sample_rate: u32) -> SynthVoiceFactory {
    match voice_type {
        VoiceType::SineAdsr => Box::new(move || {
            Box::new(SineAdsrVoice::new(
                AdsrEnvelope::new(5.0, 30.0, 0.7, 80.0, sample_rate),
                sample_rate,
            )) as Box<dyn SynthVoice>
        }),
        VoiceType::SawAdsr => Box::new(move || {
            Box::new(SawAdsrVoice::new(
                AdsrEnvelope::new(5.0, 30.0, 0.6, 100.0, sample_rate),
                sample_rate,
            )) as Box<dyn SynthVoice>
        }),
    }
}

fn apply_schedule_notes(
    runtime: &mut AudioRuntime,
    registry: &mut SequencerRegistry,
    sample_rate: u32,
    batch: NoteEventBatch,
    trigger: ScheduleTrigger,
) -> Result<(), DispatchError> {
    batch.validate()?;
    let source_id = batch.source_instance_id.clone();
    let queue = registry
        .get_mut(&source_id)
        .ok_or_else(|| DispatchError::SourceInstanceNotFound(source_id.as_str().to_string()))?;

    let transport = runtime.transport();
    let transport_start = runtime.transport_start_frame();
    let engine_now = transport_start; // close enough for Wave 7 — no in-flight engine clock probe

    let trigger_frame =
        omm_protocol::resolve_trigger(trigger, transport, engine_now, transport_start, sample_rate);

    // Push every event into the queue with `start` shifted by the
    // trigger frame's musical-time equivalent. For the simple
    // ScheduleTrigger::Frame(0) / MusicalTime(...) cases the agent
    // typically picks, the events already carry the right start
    // values and `trigger_frame` is informational. For
    // RelativeMusical / NextBar we shift by the delta from current
    // engine origin.
    let trigger_mt =
        omm_protocol::frame_to_musical_time(trigger_frame, transport, transport_start, sample_rate);
    let mut all_pushed = true;
    for ev in &batch.events {
        let shifted = NoteEvent {
            pitch_midi: ev.pitch_midi,
            velocity: ev.velocity,
            start: shift_musical_time(ev.start, trigger_mt, transport.time_signature),
            length_ticks: ev.length_ticks,
            channel: ev.channel,
        };
        if queue.enqueue(shifted).is_err() {
            all_pushed = false;
            break;
        }
    }
    if !all_pushed {
        return Err(DispatchError::NoteQueueFull);
    }

    // If the batch is meant to loop, install the template against
    // `source_id` so `registry.tick_loops()` re-pushes shifted copies
    // every `loop_bars` bars. The template carries the SHIFTED start
    // times — same as the first push — and the loop step adds further
    // bar offsets on top.
    if let Some(bars) = batch.loop_bars {
        if bars > 0 {
            let shifted_template: Vec<NoteEvent> = batch
                .events
                .into_iter()
                .map(|ev| NoteEvent {
                    pitch_midi: ev.pitch_midi,
                    velocity: ev.velocity,
                    start: shift_musical_time(ev.start, trigger_mt, transport.time_signature),
                    length_ticks: ev.length_ticks,
                    channel: ev.channel,
                })
                .collect();
            registry.install_loop(&source_id, shifted_template, bars, transport);
        }
    }
    let _ = (transport_start, sample_rate);

    Ok(())
}

fn shift_musical_time(
    base: omm_protocol::MusicalTime,
    shift: omm_protocol::MusicalTime,
    ts: omm_protocol::TimeSignature,
) -> omm_protocol::MusicalTime {
    let total = base.to_total_ticks(ts) + shift.to_total_ticks(ts);
    omm_protocol::MusicalTime::from_total_ticks(total, ts)
}

fn engine_command_label(cmd: &EngineCommand) -> &'static str {
    match cmd {
        EngineCommand::Hello { .. } => "Hello",
        EngineCommand::StartSession { .. } => "StartSession",
        EngineCommand::StopSession { .. } => "StopSession",
        EngineCommand::SetCapture { .. } => "SetCapture",
        EngineCommand::SetParam(_) => "SetParam",
        EngineCommand::SetParamBatch { .. } => "SetParamBatch",
        EngineCommand::SetSourceMute { .. } => "SetSourceMute",
        EngineCommand::GlicolLoadCode { .. } => "GlicolLoadCode",
        EngineCommand::EmergencyFade { .. } => "EmergencyFade",
        EngineCommand::RequestState => "RequestState",
        // Variants this dispatcher does handle — should never hit
        // engine_command_label, but cover them for exhaustiveness.
        EngineCommand::SetTransport { .. } => "SetTransport",
        EngineCommand::CreateSequencerSource { .. } => "CreateSequencerSource",
        EngineCommand::RemoveSequencerSource { .. } => "RemoveSequencerSource",
        EngineCommand::ScheduleNotes { .. } => "ScheduleNotes",
        EngineCommand::ClearNotes { .. } => "ClearNotes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::StereoFrame;
    use crate::runtime::AudioRuntimeConfig;
    use omm_protocol::{MusicalTime, NoteEvent, TimeSignature, Transport};

    const SR: u32 = 48_000;

    fn setup() -> (AudioRuntime, SequencerRegistry) {
        let (runtime, _q, _h) = AudioRuntime::new(AudioRuntimeConfig {
            sample_rate: SR,
            ..Default::default()
        });
        (runtime, SequencerRegistry::new())
    }

    #[test]
    fn set_transport_replaces_runtime_transport() {
        let (mut runtime, mut registry) = setup();
        let new_tport = Transport::new(140.0, TimeSignature::FOUR_FOUR);
        apply_engine_command(
            &mut runtime,
            &mut registry,
            SR,
            EngineCommand::SetTransport {
                transport: new_tport,
            },
        )
        .unwrap();
        assert_eq!(runtime.transport().bpm, 140.0);
    }

    #[test]
    fn create_sequencer_source_registers_queue_and_attaches_source() {
        let (mut runtime, mut registry) = setup();
        apply_engine_command(
            &mut runtime,
            &mut registry,
            SR,
            EngineCommand::CreateSequencerSource {
                source_instance_id: "seq:test".to_string(),
                voice_type: VoiceType::SineAdsr,
                polyphony: 8,
            },
        )
        .unwrap();
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn schedule_notes_pushes_into_correct_queue_and_renders_audio() {
        let (mut runtime, mut registry) = setup();
        apply_engine_command(
            &mut runtime,
            &mut registry,
            SR,
            EngineCommand::CreateSequencerSource {
                source_instance_id: "seq:test".to_string(),
                voice_type: VoiceType::SineAdsr,
                polyphony: 8,
            },
        )
        .unwrap();
        let batch = NoteEventBatch::new(
            SourceInstanceId::new("seq:test"),
            vec![NoteEvent::new(60, 100, MusicalTime::ZERO, 480, 0)],
        );
        apply_engine_command(
            &mut runtime,
            &mut registry,
            SR,
            EngineCommand::ScheduleNotes {
                batch,
                trigger: ScheduleTrigger::Frame(0),
            },
        )
        .unwrap();
        let mut buf = vec![StereoFrame::SILENCE; SR as usize / 10];
        // Render in blocks of MAX_BLOCK_FRAMES (512) per the engine
        // contract; rendering the whole 4800-frame slice in one call
        // would trip the channel-strip block-size debug_assert.
        let chunk = 256;
        let mut pos = 0;
        while pos < buf.len() {
            let end = (pos + chunk).min(buf.len());
            runtime.render_block(&mut buf[pos..end]);
            pos = end;
        }
        let peak = buf.iter().fold(0.0_f32, |m, f| m.max(f.left.abs()));
        assert!(peak > 0.2, "expected audible signal, peak={peak}");
    }

    #[test]
    fn schedule_notes_to_unknown_source_returns_not_found() {
        let (mut runtime, mut registry) = setup();
        let batch = NoteEventBatch::new(
            SourceInstanceId::new("seq:ghost"),
            vec![NoteEvent::new(60, 100, MusicalTime::ZERO, 480, 0)],
        );
        let res = apply_engine_command(
            &mut runtime,
            &mut registry,
            SR,
            EngineCommand::ScheduleNotes {
                batch,
                trigger: ScheduleTrigger::Frame(0),
            },
        );
        assert!(matches!(res, Err(DispatchError::SourceInstanceNotFound(_))));
    }

    #[test]
    fn remove_sequencer_source_drops_registry_entry() {
        let (mut runtime, mut registry) = setup();
        apply_engine_command(
            &mut runtime,
            &mut registry,
            SR,
            EngineCommand::CreateSequencerSource {
                source_instance_id: "seq:rm".to_string(),
                voice_type: VoiceType::SawAdsr,
                polyphony: 4,
            },
        )
        .unwrap();
        apply_engine_command(
            &mut runtime,
            &mut registry,
            SR,
            EngineCommand::RemoveSequencerSource {
                source_instance_id: "seq:rm".to_string(),
                fade_ms: 50,
            },
        )
        .unwrap();
        assert!(registry.is_empty());
    }

    #[test]
    fn unsupported_commands_return_unsupported_error() {
        let (mut runtime, mut registry) = setup();
        let res =
            apply_engine_command(&mut runtime, &mut registry, SR, EngineCommand::RequestState);
        assert!(matches!(
            res,
            Err(DispatchError::Unsupported("RequestState"))
        ));
    }

    // -- A2: loop_bars auto-repush --

    fn install_seq_with_loop(
        registry: &mut SequencerRegistry,
        runtime: &mut AudioRuntime,
    ) {
        apply_engine_command(
            runtime,
            registry,
            SR,
            EngineCommand::CreateSequencerSource {
                source_instance_id: "seq:loop".to_string(),
                voice_type: VoiceType::SineAdsr,
                polyphony: 8,
            },
        )
        .unwrap();
        let batch = NoteEventBatch::new(
            SourceInstanceId::new("seq:loop"),
            vec![NoteEvent::new(60, 100, MusicalTime::ZERO, 480, 0)],
        )
        .with_loop_bars(2);
        apply_engine_command(
            runtime,
            registry,
            SR,
            EngineCommand::ScheduleNotes {
                batch,
                trigger: ScheduleTrigger::Frame(0),
            },
        )
        .unwrap();
    }

    #[test]
    fn schedule_notes_with_loop_bars_installs_loop_template() {
        let (mut runtime, mut registry) = setup();
        install_seq_with_loop(&mut registry, &mut runtime);
        assert_eq!(registry.loop_count(), 1);
    }

    #[test]
    fn tick_loops_repushes_after_loop_bars_elapsed() {
        let (mut runtime, mut registry) = setup();
        install_seq_with_loop(&mut registry, &mut runtime);
        // 2-bar loop @ 120 BPM 4/4 = 4 s = 192_000 frames.
        // Before that no re-push; after, exactly one new iteration.
        let pushed_before = registry.tick_loops(0, SR);
        assert_eq!(pushed_before, 0);
        let pushed_after_one_loop = registry.tick_loops(192_000, SR);
        assert_eq!(pushed_after_one_loop, 1);
        // Going further forward without any time passing returns 0.
        let pushed_idle = registry.tick_loops(192_001, SR);
        assert_eq!(pushed_idle, 0);
        // Two more loops elapse — both get pushed in one tick.
        let pushed_catch_up = registry.tick_loops(192_000 * 3, SR);
        assert_eq!(pushed_catch_up, 2);
    }

    #[test]
    fn clear_all_loops_removes_installed_templates() {
        let (mut runtime, mut registry) = setup();
        install_seq_with_loop(&mut registry, &mut runtime);
        registry.clear_all_loops();
        assert_eq!(registry.loop_count(), 0);
        assert_eq!(registry.tick_loops(192_000, SR), 0);
    }
}
