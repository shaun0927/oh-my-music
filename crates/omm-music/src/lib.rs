//! omm-music — deterministic music-theory primitives.
//!
//! This crate provides the music-theory foundation (pitch, scale, chord,
//! voicing) that the LLM autonomous composition pipeline (Epic #1) uses
//! as guardrails. The LLM picks from valid options produced here instead
//! of guessing intervals or chord spellings.
//!
//! The crate is intentionally dependency-light (`serde` only) so it can
//! be used from the Rust engine or ported to TypeScript (Phase 3c)
//! without surprises.

pub mod chord;
pub mod note;
pub mod pitch;
pub mod progression;
pub mod quantize;
pub mod scale;
pub mod transform;
pub mod voicing;

pub use chord::{Chord, ChordQuality, Extension, Voicing};
pub use note::Note;
pub use pitch::{Interval, Pitch, PitchClass};
pub use progression::{generate_progression, ProgressionStyle};
pub use quantize::{quantize_to_chord, quantize_to_scale};
pub use scale::{Mode, Scale};
pub use transform::{augment, humanize, invert, retrograde, transpose};
pub use voicing::voice_leading;
