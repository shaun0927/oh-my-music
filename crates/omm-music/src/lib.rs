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
pub mod pitch;
pub mod scale;

pub use chord::{Chord, ChordQuality, Extension, Voicing};
pub use pitch::{Interval, Pitch, PitchClass};
pub use scale::{Mode, Scale};
