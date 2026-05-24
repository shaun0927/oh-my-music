use serde::{Deserialize, Serialize};

use crate::musical_time::Transport;
use crate::note_event::NoteEventBatch;
use crate::params::{ParamId, RtTarget};
use crate::scheduler::ScheduleTrigger;
use crate::source_timeline::SourceTimelineSnapshot;

/// Built-in voice flavors the agent can ask `CreateSequencerSource`
/// for. Matches `omm_audio::source::synth::{SineAdsrVoice, SawAdsrVoice}`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoiceType {
    SineAdsr,
    SawAdsr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionMode {
    Tui,
    Discord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OutputRoute {
    LocalCpal { device_id: Option<String> },
    DiscordPcmIpc { client_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SetParam {
    pub seq: u64,
    pub target: RtTarget,
    pub param: ParamId,
    pub value: f32,
    pub ramp_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EngineCommand {
    Hello {
        client_name: String,
        client_version: String,
    },
    StartSession {
        mode: SessionMode,
        output_route: OutputRoute,
    },
    StopSession {
        fade_out_ms: u32,
    },
    SetCapture {
        system_audio: bool,
        mic: bool,
        exclude_own_process: bool,
    },
    SetParam(SetParam),
    SetParamBatch {
        seq: u64,
        commands: Vec<SetParam>,
        reason: String,
    },
    SetSourceMute {
        source_instance_id: String,
        muted: bool,
        ramp_ms: u32,
    },
    GlicolLoadCode {
        code: String,
        transition_ms: u32,
    },
    EmergencyFade {
        fade_ms: u32,
        reason: String,
    },
    /// Replace the engine's master transport. Effective from the
    /// current engine frame; `transport_start_frame` resets.
    SetTransport {
        transport: Transport,
    },
    /// Spin up a new sequencer-backed generated source with the
    /// requested voice flavor and polyphony.
    CreateSequencerSource {
        source_instance_id: String,
        voice_type: VoiceType,
        polyphony: u8,
    },
    /// Stop and tear down a sequencer source (or any source instance).
    RemoveSequencerSource {
        source_instance_id: String,
        fade_ms: u32,
    },
    /// Schedule a batch of notes against a sequencer source. The
    /// trigger is resolved server-side using the master transport.
    ScheduleNotes {
        batch: NoteEventBatch,
        trigger: ScheduleTrigger,
    },
    /// Cancel pending notes on a sequencer source. `from`, if set,
    /// limits the clear to notes starting at or after that musical time.
    ClearNotes {
        source_instance_id: String,
        from: Option<crate::musical_time::MusicalTime>,
    },
    RequestState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EngineEvent {
    HelloAck {
        engine_version: String,
        sample_rate: u32,
    },
    Ack {
        acked_msg_id: u64,
        applied_seq: Option<u64>,
    },
    Nack {
        rejected_msg_id: u64,
        code: String,
        message: String,
    },
    StateSnapshot {
        engine_frame: u64,
        sample_rate: u32,
    },
    MeterFrame {
        engine_frame: u64,
        master_peak_db: f32,
        master_rms_db: f32,
    },
    CaptureStatus {
        system_audio: String,
        mic: String,
    },
    SourceTimelineSnapshot(SourceTimelineSnapshot),
}
