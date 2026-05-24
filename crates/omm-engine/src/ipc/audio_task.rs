//! Audio task — owns the `AudioRuntime` and `SequencerRegistry` on a
//! dedicated thread. Receives `EngineCommand`s via an mpsc channel,
//! applies them via the Phase 1d/2d dispatcher, sends back
//! `EngineEvent`s (Ack / Nack / StateSnapshot / SourceTimelineSnapshot)
//! over a oneshot per request.
//!
//! Between commands, pumps `render_block` so engine frame time advances
//! and any due notes actually trigger. The output is currently
//! discarded — wiring to cpal is left to a follow-up.

use omm_audio::dispatch::{apply_engine_command, DispatchError, SequencerRegistry};
use omm_audio::{
    AudioRuntime, AudioRuntimeConfig, RtCommandSchedulerError, ScheduledRtCommand, StereoFrame,
};
use omm_protocol::{EngineCommand, EngineEvent};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// One request to the audio task: a command plus the channel to send
/// the response on.
pub struct CommandEnvelope {
    pub command: EngineCommand,
    pub respond_to: oneshot::Sender<EngineEvent>,
    /// `msg_id` from the caller's envelope — echoed back in Ack/Nack
    /// so the per-connection task can correlate.
    pub msg_id: u64,
}

pub struct AudioTaskConfig {
    pub sample_rate: u32,
    pub block_frames: usize,
    pub tick_interval: Duration,
}

impl Default for AudioTaskConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            block_frames: 480, // 10 ms @ 48 kHz
            tick_interval: Duration::from_millis(10),
        }
    }
}

/// Spawn the audio thread. Returns the sender side of the command
/// channel and a handle the caller can `await` to learn the task
/// exited.
pub fn spawn_audio_task(
    config: AudioTaskConfig,
) -> (mpsc::Sender<CommandEnvelope>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<CommandEnvelope>(256);
    let handle = std::thread::Builder::new()
        .name("omm-audio".to_string())
        .spawn(move || run(config, rx))
        .expect("spawn audio thread");
    (tx, handle)
}

fn run(config: AudioTaskConfig, mut rx: mpsc::Receiver<CommandEnvelope>) {
    let (mut runtime, _command_queue, _features) = AudioRuntime::new(AudioRuntimeConfig {
        sample_rate: config.sample_rate,
        ..Default::default()
    });
    let mut registry = SequencerRegistry::new();
    let mut scratch = vec![StereoFrame::SILENCE; config.block_frames];

    // Run a small busy loop: drain pending commands without blocking,
    // pump audio every `tick_interval`. The mpsc rx provides
    // back-pressure (capacity 256) so a misbehaving client can't OOM
    // the engine.
    let mut last_render = std::time::Instant::now();
    loop {
        // Drain any pending commands without blocking.
        loop {
            match rx.try_recv() {
                Ok(envelope) => {
                    let response = handle_command(
                        &mut runtime,
                        &mut registry,
                        config.sample_rate,
                        envelope.msg_id,
                        envelope.command,
                    );
                    let _ = envelope.respond_to.send(response);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => return,
            }
        }
        // Render one block if it's time.
        let now = std::time::Instant::now();
        if now.duration_since(last_render) >= config.tick_interval {
            runtime.render_block(&mut scratch);
            last_render = now;
        } else {
            // Be polite — sleep until next tick or next command.
            std::thread::sleep(Duration::from_micros(500));
        }
    }
}

fn handle_command(
    runtime: &mut AudioRuntime,
    registry: &mut SequencerRegistry,
    sample_rate: u32,
    msg_id: u64,
    command: EngineCommand,
) -> EngineEvent {
    match &command {
        EngineCommand::Hello {
            client_name: _,
            client_version: _,
        } => EngineEvent::HelloAck {
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            sample_rate,
        },
        EngineCommand::RequestState => EngineEvent::StateSnapshot {
            engine_frame: runtime.source_timeline_snapshot().engine_frame,
            sample_rate,
        },
        _ => match apply_engine_command(runtime, registry, sample_rate, command) {
            Ok(()) => EngineEvent::Ack {
                acked_msg_id: msg_id,
                applied_seq: None,
            },
            Err(err) => EngineEvent::Nack {
                rejected_msg_id: msg_id,
                code: nack_code(&err).to_string(),
                message: err.to_string(),
            },
        },
    }
}

fn nack_code(err: &DispatchError) -> &'static str {
    match err {
        DispatchError::Unsupported(_) => "Unsupported",
        DispatchError::SourceInstanceNotFound(_) => "SourceInstanceNotFound",
        DispatchError::DuplicateSourceInstance(_) => "DuplicateSourceInstance",
        DispatchError::NoteValidation(_) => "NoteValidation",
        DispatchError::SourceInstance(_) => "SourceInstance",
        DispatchError::NoteQueueFull => "NoteQueueFull",
    }
}

// Silence unused-import warnings when scheduler types end up unused.
#[allow(dead_code)]
fn _proof_scheduler_types_compile(
    _x: Option<ScheduledRtCommand>,
    _y: Option<RtCommandSchedulerError>,
) {
}
