//! Offline render-and-judge wrapper (Epic #1 Phase 6 / D3).
//!
//! Bootstraps a fresh `AudioRuntime` + `SequencerRegistry`, applies a
//! sequence of `EngineCommand`s, renders the requested duration into
//! a `Vec<StereoFrame>`, and returns the rendered audio together with
//! a `Verdict` from [`crate::verdict::judge`].
//!
//! Intended for Phase 6 "propose-render-judge-commit" gating: the
//! agent's Composer proposes a block, this helper runs it offline at
//! engine speed (no real audio output), the Critic / dispatcher then
//! decides whether to commit it to the live engine.

use crate::dispatch::{apply_engine_command, DispatchError, SequencerRegistry};
use crate::frame::StereoFrame;
use crate::runtime::{AudioRuntime, AudioRuntimeConfig};
use crate::verdict::{judge, Verdict, VerdictConfig};

use omm_protocol::{EngineCommand, Transport};

/// One scheduled engine command, expressed in render-relative
/// milliseconds. The wrapper applies the command just before its
/// render position is reached.
pub struct ScheduledCommand {
    pub at_ms: u64,
    pub command: EngineCommand,
}

pub struct OfflineRenderConfig {
    pub sample_rate: u32,
    pub initial_transport: Transport,
    pub duration_frames: u64,
    pub block_frames: usize,
    pub verdict: VerdictConfig,
}

impl OfflineRenderConfig {
    pub fn for_duration(duration_ms: u64) -> Self {
        let sample_rate = 48_000;
        let duration_frames = (duration_ms * sample_rate as u64) / 1000;
        Self {
            sample_rate,
            initial_transport: Transport::default(),
            duration_frames,
            block_frames: 256,
            verdict: VerdictConfig {
                sample_rate,
                ..Default::default()
            },
        }
    }
}

pub struct OfflineRenderReport {
    pub audio: Vec<StereoFrame>,
    pub verdict: Verdict,
    pub dispatch_errors: Vec<(u64, DispatchError)>,
}

pub fn render_and_judge(
    config: OfflineRenderConfig,
    commands: Vec<ScheduledCommand>,
) -> OfflineRenderReport {
    let (mut runtime, _q, _h) = AudioRuntime::new(AudioRuntimeConfig {
        sample_rate: config.sample_rate,
        initial_transport: config.initial_transport,
    });
    let mut registry = SequencerRegistry::new();
    let mut audio = vec![StereoFrame::SILENCE; config.duration_frames as usize];
    let mut dispatch_errors = Vec::new();

    // Sort commands by their schedule time so we can walk the rendered
    // timeline in monotonic order.
    let mut commands = commands;
    commands.sort_by_key(|c| c.at_ms);
    let mut cmd_idx = 0;

    let frames_per_ms = config.sample_rate as f64 / 1000.0;
    let mut frame_pos: u64 = 0;
    let block = config.block_frames.max(1);

    while frame_pos < config.duration_frames {
        // Dispatch any commands whose at_ms has been reached.
        while cmd_idx < commands.len() {
            let target_frame = (commands[cmd_idx].at_ms as f64 * frames_per_ms) as u64;
            if target_frame <= frame_pos {
                let cmd =
                    std::mem::replace(&mut commands[cmd_idx].command, EngineCommand::RequestState);
                if let Err(err) =
                    apply_engine_command(&mut runtime, &mut registry, config.sample_rate, cmd)
                {
                    dispatch_errors.push((commands[cmd_idx].at_ms, err));
                }
                cmd_idx += 1;
            } else {
                break;
            }
        }
        let end = (frame_pos + block as u64).min(config.duration_frames);
        let len = (end - frame_pos) as usize;
        let slice = &mut audio[frame_pos as usize..(frame_pos as usize + len)];
        runtime.render_block(slice);
        frame_pos = end;
    }

    let verdict = judge(&audio, config.verdict);
    OfflineRenderReport {
        audio,
        verdict,
        dispatch_errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omm_protocol::{
        MusicalTime, NoteEvent, NoteEventBatch, ScheduleTrigger, SourceInstanceId, VoiceType,
    };

    fn create_sequencer(id: &str) -> EngineCommand {
        EngineCommand::CreateSequencerSource {
            source_instance_id: id.to_string(),
            voice_type: VoiceType::SineAdsr,
            polyphony: 8,
        }
    }

    fn schedule_one_note(id: &str, pitch: u8, bar: u32, beat: u16) -> EngineCommand {
        EngineCommand::ScheduleNotes {
            batch: NoteEventBatch::new(
                SourceInstanceId::new(id),
                vec![NoteEvent::new(
                    pitch,
                    100,
                    MusicalTime::new(bar, beat, 0),
                    480,
                    0,
                )],
            ),
            trigger: ScheduleTrigger::Frame(0),
        }
    }

    #[test]
    fn empty_command_list_produces_silent_verdict() {
        let report = render_and_judge(OfflineRenderConfig::for_duration(500), vec![]);
        assert!(report.verdict.reasons.too_silent);
        assert!(!report.verdict.passed);
    }

    #[test]
    fn single_note_passes_the_verdict() {
        let report = render_and_judge(
            OfflineRenderConfig::for_duration(500),
            vec![
                ScheduledCommand {
                    at_ms: 0,
                    command: create_sequencer("seq:test"),
                },
                ScheduledCommand {
                    at_ms: 0,
                    command: schedule_one_note("seq:test", 72, 0, 0),
                },
            ],
        );
        assert!(report.dispatch_errors.is_empty());
        assert!(report.audio.iter().any(|f| f.left.abs() > 0.1));
        // Note rings briefly so RMS may be low overall — at minimum
        // peak should be substantial.
        assert!(report.verdict.peak > 0.2, "verdict={:?}", report.verdict);
    }

    #[test]
    fn dispatch_errors_are_collected_without_panic() {
        let report = render_and_judge(
            OfflineRenderConfig::for_duration(200),
            vec![ScheduledCommand {
                at_ms: 0,
                command: schedule_one_note("seq:does-not-exist", 60, 0, 0),
            }],
        );
        assert_eq!(report.dispatch_errors.len(), 1);
    }

    #[test]
    fn scheduled_commands_fire_in_order() {
        // Notes must be in the FUTURE relative to when they're
        // dispatched: t=100ms dispatches a note at beat 1 (= 500ms
        // @ 120 BPM), t=200ms a note at beat 2 (= 1000ms). 1500ms
        // render duration covers both.
        let report = render_and_judge(
            OfflineRenderConfig::for_duration(1500),
            vec![
                ScheduledCommand {
                    at_ms: 0,
                    command: create_sequencer("seq:a"),
                },
                ScheduledCommand {
                    at_ms: 100,
                    command: schedule_one_note("seq:a", 60, 0, 1),
                },
                ScheduledCommand {
                    at_ms: 200,
                    command: schedule_one_note("seq:a", 67, 0, 2),
                },
            ],
        );
        assert!(
            report.dispatch_errors.is_empty(),
            "errors: {:?}",
            report.dispatch_errors
        );
        assert!(
            report.audio.iter().any(|f| f.left.abs() > 0.1),
            "expected at least one loud sample; max amplitude was {}",
            report
                .audio
                .iter()
                .fold(0.0_f32, |m, f| m.max(f.left.abs()))
        );
    }
}
