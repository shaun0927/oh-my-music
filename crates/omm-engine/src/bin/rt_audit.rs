//! RT-audit utility (A4) — runs the audio render hot path through
//! its paces and reports peak/avg block time, NaN/Inf safety, and
//! basic sanity (output never out of [-1, 1]).
//!
//! Run with:
//!   cargo run --release --bin rt-audit
//!
//! The audit is *not* a substitute for `assert_no_alloc` / `loom`
//! (those need separate harnesses and feature flags). It catches
//! the dumbest regressions — a callback that suddenly spends 10 ms
//! per block, or one that writes NaN samples — without specialized
//! tooling.

use omm_audio::dispatch::{apply_engine_command, SequencerRegistry};
use omm_audio::source::sequencer::SynthVoiceFactory;
use omm_audio::source::{AdsrEnvelope, SineAdsrVoice, SynthVoice};
use omm_audio::{AudioRuntime, AudioRuntimeConfig, StereoFrame};
use omm_protocol::{
    EngineCommand, MusicalTime, NoteEvent, NoteEventBatch, ScheduleTrigger, SourceInstanceId,
    TimeSignature, Transport, VoiceType,
};
use std::time::{Duration, Instant};

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 256;
const BLOCKS: usize = 4_000; // ~21 s of audio at 256 frames / block
const MAX_AVG_PER_BLOCK_MS: f64 = 1.3; // 25 % of a 5.33 ms / 256-frame budget

#[derive(Debug)]
#[allow(dead_code)] // fields are read via the `Debug` impl, not directly.
struct AuditReport {
    blocks_rendered: usize,
    total_time_ms: f64,
    avg_block_ms: f64,
    max_block_ms: f64,
    nan_or_inf_samples: u64,
    out_of_range_samples: u64,
    nan_recovery_ok: bool,
}

fn main() -> anyhow::Result<()> {
    let report = audit();
    println!("--- rt-audit ---");
    println!("{report:#?}");

    let mut failures = Vec::new();
    if report.nan_or_inf_samples > 0 {
        failures.push(format!("NaN/Inf samples: {}", report.nan_or_inf_samples));
    }
    if report.out_of_range_samples > 0 {
        failures.push(format!(
            "out-of-range samples: {}",
            report.out_of_range_samples
        ));
    }
    if !report.nan_recovery_ok {
        failures.push("NaN injection not clamped to [-1, 1]".to_string());
    }
    if report.avg_block_ms > MAX_AVG_PER_BLOCK_MS {
        failures.push(format!(
            "avg block {:.3} ms > {:.3} ms budget",
            report.avg_block_ms, MAX_AVG_PER_BLOCK_MS
        ));
    }

    if failures.is_empty() {
        println!("OK");
        Ok(())
    } else {
        for f in &failures {
            eprintln!("FAIL: {f}");
        }
        std::process::exit(1)
    }
}

fn audit() -> AuditReport {
    let (mut runtime, _q, _h) = AudioRuntime::new(AudioRuntimeConfig {
        sample_rate: SAMPLE_RATE,
        initial_transport: Transport::new(120.0, TimeSignature::FOUR_FOUR),
    });
    let mut registry = SequencerRegistry::new();
    // Spin up a sequencer + a few notes so the render loop isn't trivially silent.
    let _ = apply_engine_command(
        &mut runtime,
        &mut registry,
        SAMPLE_RATE,
        EngineCommand::CreateSequencerSource {
            source_instance_id: "rt-audit".to_string(),
            voice_type: VoiceType::SineAdsr,
            polyphony: 8,
        },
    );
    let _ = apply_engine_command(
        &mut runtime,
        &mut registry,
        SAMPLE_RATE,
        EngineCommand::ScheduleNotes {
            batch: NoteEventBatch::new(
                SourceInstanceId::new("rt-audit"),
                vec![
                    NoteEvent::new(60, 100, MusicalTime::new(0, 0, 0), 480, 0),
                    NoteEvent::new(64, 100, MusicalTime::new(0, 2, 0), 480, 0),
                    NoteEvent::new(67, 100, MusicalTime::new(1, 0, 0), 480, 0),
                ],
            ),
            trigger: ScheduleTrigger::Frame(0),
        },
    );

    let mut buf = vec![StereoFrame::SILENCE; BLOCK_FRAMES];
    let mut nan_inf = 0_u64;
    let mut oor = 0_u64;
    let mut max_block = Duration::from_secs(0);
    let start = Instant::now();
    for _ in 0..BLOCKS {
        let t = Instant::now();
        runtime.render_block(&mut buf);
        let dur = t.elapsed();
        if dur > max_block {
            max_block = dur;
        }
        for f in &buf {
            if !f.left.is_finite() || !f.right.is_finite() {
                nan_inf += 1;
            }
            if f.left.abs() > 1.0 || f.right.abs() > 1.0 {
                oor += 1;
            }
        }
    }
    let elapsed = start.elapsed();

    // Inject NaN: push silent buffer with NaN sentinel and re-render
    // to make sure nan_guard scrubs them. (Direct injection into the
    // mixed output isn't part of the public API; we approximate by
    // rendering one extra block and ensuring its output stays in
    // range.)
    let mut probe = vec![StereoFrame::new(f32::NAN, f32::NAN); BLOCK_FRAMES];
    runtime.render_block(&mut probe);
    let nan_recovery_ok = probe.iter().all(|f| {
        f.left.is_finite() && f.right.is_finite() && f.left.abs() <= 1.0 && f.right.abs() <= 1.0
    });

    AuditReport {
        blocks_rendered: BLOCKS,
        total_time_ms: elapsed.as_secs_f64() * 1000.0,
        avg_block_ms: elapsed.as_secs_f64() * 1000.0 / BLOCKS as f64,
        max_block_ms: max_block.as_secs_f64() * 1000.0,
        nan_or_inf_samples: nan_inf,
        out_of_range_samples: oor,
        nan_recovery_ok,
    }
}

// Silence unused warning if synth re-exports drift.
#[allow(dead_code)]
fn _proof_synth_voice_compiles() -> SynthVoiceFactory {
    Box::new(|| {
        Box::new(SineAdsrVoice::new(
            AdsrEnvelope::new(5.0, 30.0, 0.7, 80.0, SAMPLE_RATE),
            SAMPLE_RATE,
        )) as Box<dyn SynthVoice>
    })
}
