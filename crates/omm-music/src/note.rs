use serde::{Deserialize, Serialize};

use crate::pitch::Pitch;

/// A simple in-crate note record used by the generators / transforms
/// in `progression`, `quantize`, `transform`, `humanize`. Distinct
/// from `omm_protocol::NoteEvent` so this crate stays free of the
/// protocol dep. A thin adapter (Phase 3c / agent layer) bridges
/// between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Note {
    pub pitch: Pitch,
    pub velocity: u8,
    pub start_ticks: u32,
    pub length_ticks: u32,
}

impl Note {
    pub const fn new(pitch: Pitch, velocity: u8, start_ticks: u32, length_ticks: u32) -> Self {
        Self {
            pitch,
            velocity,
            start_ticks,
            length_ticks,
        }
    }
}
