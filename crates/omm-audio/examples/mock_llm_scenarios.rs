//! Phase 1d/2d Integration Gate I5 evidence.
//!
//! Drives the engine through the 5 mock-LLM scenarios described in
//! issue #11 and writes one WAV per scenario:
//!
//! 1. C major 120 BPM I-V-vi-IV (4 bars)
//! 2. A minor 90 BPM modal Dorian (8 bars, walking bass)
//! 3. F major 140 BPM beat + bassline (16 bars)
//! 4. E phrygian 100 BPM melody + chords (4 bars)
//! 5. invalid lead-time → Nack with guidance text
//!
//! The "mock LLM" lives inside the example: each scenario builds a
//! sequence of `EngineCommand`s that the real agent would emit, then
//! pumps them through the dispatcher. Output WAVs land in `/tmp/`.
//!
//! Run with:
//!   cargo run -p omm-audio --example mock_llm_scenarios

use omm_audio::dispatch::{apply_engine_command, DispatchError, SequencerRegistry};
use omm_audio::{AudioRuntime, AudioRuntimeConfig, StereoFrame};
use omm_protocol::{
    ActionOrigin, EngineCommand, MusicalTime, NoteEvent, NoteEventBatch, ScheduleRequestTiming,
    ScheduleTrigger, ScheduleValidationError, ScheduledActionId, SourceInstanceId, TimeSignature,
    Transport, VoiceType, PLANNED_ACTION_MIN_LEAD_MS,
};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK: usize = 256;

fn main() -> std::io::Result<()> {
    let scenarios: [(&str, ScenarioFn); 5] = [
        ("c_major_pop", scenario_c_major_pop as ScenarioFn),
        ("a_minor_dorian", scenario_a_minor_dorian as ScenarioFn),
        ("f_major_beat", scenario_f_major_beat as ScenarioFn),
        ("e_phrygian_melody", scenario_e_phrygian_melody as ScenarioFn),
        ("lead_time_violation", scenario_lead_time_violation as ScenarioFn),
    ];

    for (slug, scenario) in scenarios {
        match scenario() {
            Ok(audio) => {
                let path = PathBuf::from(format!("/tmp/mock_llm_{slug}.wav"));
                write_wav_pcm16(&path, &audio, SAMPLE_RATE)?;
                let peak = audio
                    .iter()
                    .fold(0.0_f32, |m, f| m.max(f.left.abs()).max(f.right.abs()));
                println!(
                    "[{slug}] wrote {:.2}s, peak {:.3} → {}",
                    audio.len() as f32 / SAMPLE_RATE as f32,
                    peak,
                    path.display()
                );
            }
            Err(text) => {
                println!("[{slug}] (no audio) {text}");
            }
        }
    }

    println!(
        "I5 done. Audition WAVs in /tmp/mock_llm_*.wav and confirm 4 of 5 are musically sane;"
    );
    println!("the 5th scenario must print a clear lead-time guidance message instead of writing audio.");
    Ok(())
}

type ScenarioFn = fn() -> Result<Vec<StereoFrame>, String>;

fn setup(transport: Transport) -> (AudioRuntime, SequencerRegistry) {
    let (rt, _q, _h) = AudioRuntime::new(AudioRuntimeConfig {
        sample_rate: SAMPLE_RATE,
        initial_transport: transport,
    });
    (rt, SequencerRegistry::new())
}

fn render(runtime: &mut AudioRuntime, frames: usize) -> Vec<StereoFrame> {
    let mut out = vec![StereoFrame::SILENCE; frames];
    let mut pos = 0;
    while pos < frames {
        let end = (pos + BLOCK).min(frames);
        runtime.render_block(&mut out[pos..end]);
        pos = end;
    }
    out
}

fn pump(
    runtime: &mut AudioRuntime,
    registry: &mut SequencerRegistry,
    cmds: Vec<EngineCommand>,
) -> Result<(), DispatchError> {
    for cmd in cmds {
        apply_engine_command(runtime, registry, SAMPLE_RATE, cmd)?;
    }
    Ok(())
}

// --- Scenario 1: C major 120 BPM I-V-vi-IV, 4 bars ----------------------

fn scenario_c_major_pop() -> Result<Vec<StereoFrame>, String> {
    let (mut rt, mut reg) = setup(Transport::new(120.0, TimeSignature::FOUR_FOUR));
    let chord_roots = [60_u8, 67, 69, 65]; // C, G, A, F
    let mut events = Vec::new();
    for (bar, &root) in chord_roots.iter().enumerate() {
        // Chord stab on beat 0 (root + third + fifth, sustained for the bar).
        for offset in [0_u8, 4, 7] {
            events.push(NoteEvent::new(
                root + offset,
                90,
                MusicalTime::new(bar as u32, 0, 0),
                1920,
                0,
            ));
        }
        // Quarter-note arpeggio on beats 1..3.
        for (i, offset) in [4_u8, 7, 12].iter().enumerate() {
            events.push(NoteEvent::new(
                root + offset,
                80,
                MusicalTime::new(bar as u32, (i + 1) as u16, 0),
                480,
                0,
            ));
        }
    }
    // Validator requires sorted-by-start; sort by (bar, beat) which is
    // exactly MusicalTime's lexicographic order.
    events.sort_by(|a, b| a.start.cmp(&b.start));
    let cmds = vec![
        EngineCommand::CreateSequencerSource {
            source_instance_id: "seq:pop".to_string(),
            voice_type: VoiceType::SineAdsr,
            polyphony: 8,
        },
        EngineCommand::ScheduleNotes {
            batch: NoteEventBatch::new(SourceInstanceId::new("seq:pop"), events),
            trigger: ScheduleTrigger::Frame(0),
        },
    ];
    pump(&mut rt, &mut reg, cmds).map_err(|e| e.to_string())?;
    Ok(render(&mut rt, SAMPLE_RATE as usize * 9)) // ~9s (8s music + tail)
}

// --- Scenario 2: A minor 90 BPM modal Dorian, 8 bars --------------------

fn scenario_a_minor_dorian() -> Result<Vec<StereoFrame>, String> {
    let (mut rt, mut reg) = setup(Transport::new(90.0, TimeSignature::FOUR_FOUR));
    // Walking bass A2 (45) → D3 (50) → G3 (55) → A2 → ...
    let scale = [45_u8, 47, 48, 50, 52, 53, 55, 57]; // A natural minor low octave
    let mut events = Vec::new();
    for bar in 0..8_u32 {
        for beat in 0..4_u16 {
            let idx = ((bar * 4 + beat as u32) as usize) % scale.len();
            events.push(NoteEvent::new(
                scale[idx],
                85,
                MusicalTime::new(bar, beat, 0),
                480,
                0,
            ));
        }
    }
    let cmds = vec![
        EngineCommand::CreateSequencerSource {
            source_instance_id: "seq:dorian".to_string(),
            voice_type: VoiceType::SawAdsr,
            polyphony: 4,
        },
        EngineCommand::ScheduleNotes {
            batch: NoteEventBatch::new(SourceInstanceId::new("seq:dorian"), events),
            trigger: ScheduleTrigger::Frame(0),
        },
    ];
    pump(&mut rt, &mut reg, cmds).map_err(|e| e.to_string())?;
    Ok(render(&mut rt, (SAMPLE_RATE as usize * 23) / 1)) // 8 bars @ 90bpm = ~21s; +2s tail
}

// --- Scenario 3: F major 140 BPM beat + bassline, 16 bars --------------

fn scenario_f_major_beat() -> Result<Vec<StereoFrame>, String> {
    let (mut rt, mut reg) = setup(Transport::new(140.0, TimeSignature::FOUR_FOUR));
    // Bass on every downbeat (1, 3) of each bar at F2 (41).
    // Kick alternative: high pitch (high pop note) on every quarter as
    // a rhythmic marker so the listener perceives 4/4.
    let mut bass_events = Vec::new();
    let mut beat_events = Vec::new();
    for bar in 0..16_u32 {
        // bass: F2 on beats 0 and 2
        for &b in &[0_u16, 2] {
            bass_events.push(NoteEvent::new(41, 110, MusicalTime::new(bar, b, 0), 480, 0));
        }
        // beat: a single high "tick" pitch on every beat
        for b in 0..4_u16 {
            beat_events.push(NoteEvent::new(96, 70, MusicalTime::new(bar, b, 0), 120, 0));
        }
    }
    let cmds = vec![
        EngineCommand::CreateSequencerSource {
            source_instance_id: "seq:bass".to_string(),
            voice_type: VoiceType::SawAdsr,
            polyphony: 4,
        },
        EngineCommand::CreateSequencerSource {
            source_instance_id: "seq:beat".to_string(),
            voice_type: VoiceType::SineAdsr,
            polyphony: 4,
        },
        EngineCommand::ScheduleNotes {
            batch: NoteEventBatch::new(SourceInstanceId::new("seq:bass"), bass_events),
            trigger: ScheduleTrigger::Frame(0),
        },
        EngineCommand::ScheduleNotes {
            batch: NoteEventBatch::new(SourceInstanceId::new("seq:beat"), beat_events),
            trigger: ScheduleTrigger::Frame(0),
        },
    ];
    pump(&mut rt, &mut reg, cmds).map_err(|e| e.to_string())?;
    // 16 bars @ 140bpm = (16 * 4 / 140 * 60) = ~27.4s
    Ok(render(&mut rt, SAMPLE_RATE as usize * 29))
}

// --- Scenario 4: E phrygian 100 BPM melody + chords, 4 bars ------------

fn scenario_e_phrygian_melody() -> Result<Vec<StereoFrame>, String> {
    let (mut rt, mut reg) = setup(Transport::new(100.0, TimeSignature::FOUR_FOUR));
    // E phrygian: E F G A B C D E   (MIDI offsets from E4=64)
    let melody = [64, 65, 67, 69, 71, 72, 74, 76];
    let mut melody_events = Vec::new();
    let mut chord_events = Vec::new();
    for bar in 0..4_u32 {
        for beat in 0..4_u16 {
            let idx = ((bar * 4 + beat as u32) as usize) % melody.len();
            melody_events.push(NoteEvent::new(
                melody[idx],
                95,
                MusicalTime::new(bar, beat, 0),
                480,
                0,
            ));
        }
        // E minor chord on each downbeat: E G B
        for pitch in [52_u8, 55, 59] {
            chord_events.push(NoteEvent::new(pitch, 70, MusicalTime::new(bar, 0, 0), 1920, 0));
        }
    }
    let cmds = vec![
        EngineCommand::CreateSequencerSource {
            source_instance_id: "seq:phr-melody".to_string(),
            voice_type: VoiceType::SineAdsr,
            polyphony: 4,
        },
        EngineCommand::CreateSequencerSource {
            source_instance_id: "seq:phr-chords".to_string(),
            voice_type: VoiceType::SawAdsr,
            polyphony: 4,
        },
        EngineCommand::ScheduleNotes {
            batch: NoteEventBatch::new(SourceInstanceId::new("seq:phr-melody"), melody_events),
            trigger: ScheduleTrigger::Frame(0),
        },
        EngineCommand::ScheduleNotes {
            batch: NoteEventBatch::new(SourceInstanceId::new("seq:phr-chords"), chord_events),
            trigger: ScheduleTrigger::Frame(0),
        },
    ];
    pump(&mut rt, &mut reg, cmds).map_err(|e| e.to_string())?;
    // 4 bars @ 100bpm = 9.6s
    Ok(render(&mut rt, SAMPLE_RATE as usize * 11))
}

// --- Scenario 5: lead-time violation ------------------------------------

fn scenario_lead_time_violation() -> Result<Vec<StereoFrame>, String> {
    let transport = Transport::new(120.0, TimeSignature::FOUR_FOUR);
    // Validate at the protocol layer: a PlannedLlm trigger of 2 bars
    // (2 seconds @ 120 BPM) must be rejected as too soon — must be
    // >= 30 seconds. The agent shall present this Nack to the LLM
    // with the explicit minimum.
    let trigger_frame = omm_protocol::resolve_trigger(
        ScheduleTrigger::RelativeMusical { bars: 2, beats: 0 },
        transport,
        0,
        0,
        SAMPLE_RATE,
    );
    let validation = omm_protocol::validate_schedule_request(
        ScheduledActionId::new("planned-too-soon"),
        ActionOrigin::PlannedLlm,
        ScheduleRequestTiming {
            submitted_at_frame: 0,
            trigger_frame,
        },
        SAMPLE_RATE,
    );
    match validation {
        Err(ScheduleValidationError::PlannedActionTooSoon {
            trigger_frame,
            minimum_trigger_frame,
        }) => Err(format!(
            "Nack: PlannedActionTooSoon (trigger={trigger_frame}, minimum={minimum_trigger_frame} = {}ms). \
             Agent guidance: use ScheduleTrigger::RelativeMusical {{ bars: >=15, beats: 0 }} at 120 BPM 4/4 \
             so the resolved frame satisfies the {PLANNED_ACTION_MIN_LEAD_MS}ms lead-time guard.",
            PLANNED_ACTION_MIN_LEAD_MS,
        )),
        Err(other) => Err(format!("unexpected error: {other:?}")),
        Ok(_) => Err("validation unexpectedly passed".to_string()),
    }
}

fn write_wav_pcm16(
    path: &PathBuf,
    frames: &[StereoFrame],
    sample_rate: u32,
) -> std::io::Result<()> {
    let n_channels: u16 = 2;
    let bits: u16 = 16;
    let n_frames = frames.len() as u32;
    let byte_rate = sample_rate * n_channels as u32 * (bits as u32 / 8);
    let block_align = n_channels * bits / 8;
    let data_bytes = n_frames * block_align as u32;

    let mut w = BufWriter::new(File::create(path)?);
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16_u32.to_le_bytes())?;
    w.write_all(&1_u16.to_le_bytes())?;
    w.write_all(&n_channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&bits.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    for f in frames {
        let l = (f.left.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        let r = (f.right.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        w.write_all(&l.to_le_bytes())?;
        w.write_all(&r.to_le_bytes())?;
    }
    w.flush()?;
    Ok(())
}
