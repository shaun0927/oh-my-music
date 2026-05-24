//! Render a 1-second A4 sine tone via `SineAdsrVoice` and write a WAV
//! file. Provides the Tier 2 Domain Evidence Artifact for issue #7
//! (Phase 2c). Run with:
//!
//! ```text
//! cargo run -p omm-audio --example synth_a4
//! ```
//!
//! Output: `/tmp/synth_a4.wav` (1 second, 48 kHz stereo, 16-bit PCM).

use omm_audio::source::{AdsrEnvelope, SineAdsrVoice, SynthVoice};
use omm_audio::StereoFrame;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

const SAMPLE_RATE: u32 = 48_000;

fn main() -> std::io::Result<()> {
    let env = AdsrEnvelope::new(5.0, 50.0, 0.7, 200.0, SAMPLE_RATE);
    let mut voice = SineAdsrVoice::new(env, SAMPLE_RATE);
    voice.note_on(69, 100); // A4 at velocity 100

    // 1 s total: 800 ms note held + 200 ms release tail.
    let total = SAMPLE_RATE as usize;
    let hold = (SAMPLE_RATE as usize * 8) / 10;
    let mut buf = vec![StereoFrame::SILENCE; total];
    voice.render(&mut buf[..hold]);
    voice.note_off(69);
    voice.render(&mut buf[hold..]);

    let path = PathBuf::from("/tmp/synth_a4.wav");
    write_wav_pcm16(&path, &buf, SAMPLE_RATE)?;

    let peak = buf
        .iter()
        .fold(0.0_f32, |m, f| m.max(f.left.abs()).max(f.right.abs()));
    println!(
        "wrote {} frames ({} bytes raw audio) to {} (peak amplitude {:.3})",
        buf.len(),
        buf.len() * 4,
        path.display(),
        peak
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
