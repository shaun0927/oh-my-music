//! Phase 2b Integration Gate I3 evidence: drive a `SequencerSource`
//! end-to-end through the engine with note events composed in code
//! (no agent / LLM), and dump the resulting audio to a WAV file.
//!
//! Run with:
//!   cargo run -p omm-audio --example agentless_compose
//!
//! Output: `/tmp/agentless_compose.wav` — C major 1-octave 8-note
//! ascending arpeggio at 120 BPM 4/4, eighth notes, ~2 seconds.

use omm_audio::source::{AdsrEnvelope, SineAdsrVoice, SynthVoice, SynthVoiceFactory};
use omm_audio::{AudioRuntime, AudioRuntimeConfig, StereoFrame};
use omm_protocol::{MusicalTime, NoteEvent, SourceInstanceId, TimeSignature, Transport};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 512;

fn main() -> std::io::Result<()> {
    let transport = Transport::new(120.0, TimeSignature::FOUR_FOUR);
    let (mut runtime, _q, _h) = AudioRuntime::new(AudioRuntimeConfig {
        sample_rate: SAMPLE_RATE,
        initial_transport: transport,
    });

    let factory: SynthVoiceFactory = Box::new(|| {
        Box::new(SineAdsrVoice::new(
            AdsrEnvelope::new(5.0, 30.0, 0.7, 80.0, SAMPLE_RATE),
            SAMPLE_RATE,
        )) as Box<dyn SynthVoice>
    });

    let mut note_queue = runtime
        .add_sequencer_source(SourceInstanceId::new("seq:agentless"), factory, 16)
        .expect("sequencer source attached");

    // Push a C major ascending arpeggio: C4 D4 E4 F4 G4 A4 B4 C5,
    // eighth notes (240 ticks each), starting at bar 0 beat 0.
    let pitches = [60_u8, 62, 64, 65, 67, 69, 71, 72];
    for (i, pitch) in pitches.iter().enumerate() {
        let bar = (i / 8) as u32;
        let beat = ((i % 8) / 2) as u16;
        let tick = (((i % 8) % 2) * 240) as u16;
        let event = NoteEvent::new(*pitch, 100, MusicalTime::new(bar, beat, tick), 240, 0);
        note_queue.enqueue(event).expect("queue not full");
    }

    // Render 2.5 seconds — 1 bar at 120 BPM = 2 s, plus envelope tail.
    let total_frames = (SAMPLE_RATE as usize * 5) / 2;
    let mut audio = vec![StereoFrame::SILENCE; total_frames];
    let mut pos = 0;
    while pos < total_frames {
        let block_len = (total_frames - pos).min(BLOCK_FRAMES);
        runtime.render_block(&mut audio[pos..pos + block_len]);
        pos += block_len;
    }

    let path = PathBuf::from("/tmp/agentless_compose.wav");
    write_wav_pcm16(&path, &audio, SAMPLE_RATE)?;

    let peak = audio
        .iter()
        .fold(0.0_f32, |m, f| m.max(f.left.abs()).max(f.right.abs()));
    println!(
        "wrote {} frames ({:.2} s, peak {:.3}) to {}",
        audio.len(),
        audio.len() as f32 / SAMPLE_RATE as f32,
        peak,
        path.display()
    );
    Ok(())
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

    let file = File::create(path)?;
    let mut w = BufWriter::new(file);
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16_u32.to_le_bytes())?;
    w.write_all(&1_u16.to_le_bytes())?; // PCM
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
