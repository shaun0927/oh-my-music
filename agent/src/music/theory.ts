// TypeScript port of `crates/omm-music`. All deterministic functions
// here MUST produce the same output as the Rust crate for the same
// inputs. The inline tests in `__tests__/theory.test.ts` pin that.

import type {
  Chord,
  ChordQuality,
  Extension,
  Mode,
  Note,
  Pitch,
  PitchClass,
  ProgressionStyle,
  Scale,
  Voicing,
} from "./types";

// -- Note ↔ engine NoteEvent adapter -------------------------------------

const TICKS_PER_QUARTER = 480;

/// JSON shape used by the engine's `omm-protocol::NoteEvent` (matches
/// the schema the `schedule_notes` Pi tool emits). Kept here so callers
/// who built a melody with `transposeNotes` / `humanizeNotes` can hand
/// the result straight to `engineClient.scheduleNotes`.
export interface EngineNoteEvent {
  pitch_midi: number;
  velocity: number;
  start: { bar: number; beat: number; tick: number };
  length_ticks: number;
  channel: number;
}

/// Convert an in-crate `Note` (start/length in ticks-from-zero) to the
/// engine's `NoteEvent` shape (start as MusicalTime under the given
/// time signature). `beatsPerBar` and `ticksPerBeat` default to 4/4
/// with PPQ 480 to match the engine's defaults.
export function noteToEngineEvent(
  note: Note,
  channel = 0,
  beatsPerBar = 4,
  ticksPerBeat: number = TICKS_PER_QUARTER,
): EngineNoteEvent {
  const ticksPerBar = beatsPerBar * ticksPerBeat;
  const bar = Math.floor(note.start_ticks / ticksPerBar);
  const remBar = note.start_ticks - bar * ticksPerBar;
  const beat = Math.floor(remBar / ticksPerBeat);
  const tick = remBar - beat * ticksPerBeat;
  return {
    pitch_midi: note.pitch,
    velocity: note.velocity,
    start: { bar, beat, tick },
    length_ticks: note.length_ticks,
    channel,
  };
}

/// Batch convert.
export function notesToEngineEvents(
  notes: Note[],
  channel = 0,
  beatsPerBar = 4,
  ticksPerBeat: number = TICKS_PER_QUARTER,
): EngineNoteEvent[] {
  return notes.map((n) => noteToEngineEvent(n, channel, beatsPerBar, ticksPerBeat));
}

// -- Pitch ----------------------------------------------------------------

export function clampPitch(midi: number): Pitch {
  if (!Number.isFinite(midi)) return 0;
  return Math.max(0, Math.min(127, Math.round(midi)));
}

export function pitchToPitchClass(p: Pitch): PitchClass {
  return ((p % 12) + 12) % 12;
}

export function transposePitch(p: Pitch, semitones: Interval): Pitch | null {
  const next = p + semitones;
  return next >= 0 && next <= 127 ? next : null;
}

export function transposePitchClamped(p: Pitch, semitones: Interval): Pitch {
  return clampPitch(p + semitones);
}

export function pitchToFreqHz(p: Pitch): number {
  return 440 * Math.pow(2, (p - 69) / 12);
}

export type Interval = number;

export function pitchClassShift(pc: PitchClass, semitones: number): PitchClass {
  const v = ((pc + semitones) % 12 + 12) % 12;
  return v;
}

// -- Scale ----------------------------------------------------------------

const MODE_INTERVALS: Record<Mode, number[]> = {
  Ionian: [0, 2, 4, 5, 7, 9, 11],
  Dorian: [0, 2, 3, 5, 7, 9, 10],
  Phrygian: [0, 1, 3, 5, 7, 8, 10],
  Lydian: [0, 2, 4, 6, 7, 9, 11],
  Mixolydian: [0, 2, 4, 5, 7, 9, 10],
  Aeolian: [0, 2, 3, 5, 7, 8, 10],
  Locrian: [0, 1, 3, 5, 6, 8, 10],
};

export function modeIntervals(mode: Mode): number[] {
  return MODE_INTERVALS[mode];
}

export function scalePitchClasses(scale: Scale): PitchClass[] {
  return modeIntervals(scale.mode).map((iv) => pitchClassShift(scale.root, iv));
}

export function scaleContains(scale: Scale, pitch: Pitch): boolean {
  return scalePitchClasses(scale).includes(pitchToPitchClass(pitch));
}

export function degreeToPitch(
  scale: Scale,
  degree: number,
  octave: number,
): Pitch | null {
  if (degree < 1 || degree > 7) return null;
  const pc = scalePitchClasses(scale)[degree - 1];
  const midi = (octave + 1) * 12 + pc;
  return midi >= 0 && midi <= 127 ? midi : null;
}

// -- Chord ----------------------------------------------------------------

const CHORD_INTERVALS: Record<ChordQuality, number[]> = {
  Maj: [0, 4, 7],
  Min: [0, 3, 7],
  Dim: [0, 3, 6],
  Aug: [0, 4, 8],
  Sus2: [0, 2, 7],
  Sus4: [0, 5, 7],
  Dom7: [0, 4, 7, 10],
  Maj7: [0, 4, 7, 11],
  Min7: [0, 3, 7, 10],
  Min7b5: [0, 3, 6, 10],
  Dim7: [0, 3, 6, 9],
  MinMaj7: [0, 3, 7, 11],
};

export const ALL_CHORD_QUALITIES: ChordQuality[] = [
  "Maj",
  "Min",
  "Dim",
  "Aug",
  "Sus2",
  "Sus4",
  "Dom7",
  "Maj7",
  "Min7",
  "Min7b5",
  "Dim7",
  "MinMaj7",
];

const EXTENSION_SEMITONES: Record<Extension, number> = {
  Nine: 14,
  FlatNine: 13,
  SharpNine: 15,
  Eleven: 17,
  SharpEleven: 18,
  FlatThirteen: 20,
  Thirteen: 21,
};

export function chordIntervals(quality: ChordQuality): number[] {
  return CHORD_INTERVALS[quality];
}

export function chord(root: PitchClass, quality: ChordQuality, extensions: Extension[] = []): Chord {
  return { root, quality, extensions };
}

export function basicVoicing(c: Chord, rootOctave: number): Voicing {
  const rootMidi = (rootOctave + 1) * 12 + c.root;
  const pitches: Pitch[] = [];
  for (const iv of chordIntervals(c.quality)) {
    const m = rootMidi + iv;
    if (m >= 0 && m <= 127) pitches.push(m);
  }
  for (const ext of c.extensions) {
    const m = rootMidi + EXTENSION_SEMITONES[ext];
    if (m >= 0 && m <= 127) pitches.push(m);
  }
  pitches.sort((a, b) => a - b);
  // dedup
  const deduped: Pitch[] = [];
  for (const p of pitches) {
    if (deduped.length === 0 || deduped[deduped.length - 1] !== p) deduped.push(p);
  }
  return { pitches: deduped };
}

// -- Voice leading --------------------------------------------------------

export function voiceLeading(
  prev: Voicing,
  target: Chord,
  maxMovementSemitones: number,
  targetRootOctave: number,
): Voicing {
  const fallback = basicVoicing(target, targetRootOctave);
  const candidates = fallback.pitches.slice();
  if (candidates.length === 0) return { pitches: [] };
  const out: Pitch[] = [];
  prev.pitches.forEach((voice, i) => {
    let nearest = candidates[0];
    let nearestDist = Math.abs(nearest - voice);
    for (const c of candidates) {
      const d = Math.abs(c - voice);
      if (d < nearestDist) {
        nearest = c;
        nearestDist = d;
      }
    }
    if (nearestDist <= maxMovementSemitones) {
      out.push(nearest);
    } else {
      out.push(candidates[Math.min(i, candidates.length - 1)]);
    }
  });
  for (const c of candidates) {
    if (!out.includes(c)) out.push(c);
  }
  out.sort((a, b) => a - b);
  const deduped: Pitch[] = [];
  for (const p of out) {
    if (deduped.length === 0 || deduped[deduped.length - 1] !== p) deduped.push(p);
  }
  return { pitches: deduped };
}

// -- Progression ----------------------------------------------------------

// Match the chord-quality assignment Rust's `progression::diatonic_chord` uses.
function pcDistance(from: PitchClass, to: PitchClass): number {
  return ((to - from) % 12 + 12) % 12;
}

export function diatonicChord(scale: Scale, degree: number): Chord {
  const d = Math.max(1, Math.min(7, degree));
  const pcs = scalePitchClasses(scale);
  const root = pcs[d - 1];
  const third = pcs[(d + 1) % 7];
  const fifth = pcs[(d + 3) % 7];
  const thirdInterval = pcDistance(root, third);
  const fifthInterval = pcDistance(root, fifth);
  let quality: ChordQuality;
  if (thirdInterval === 4 && fifthInterval === 7) quality = "Maj";
  else if (thirdInterval === 3 && fifthInterval === 7) quality = "Min";
  else if (thirdInterval === 3 && fifthInterval === 6) quality = "Dim";
  else if (thirdInterval === 4 && fifthInterval === 8) quality = "Aug";
  else quality = "Maj";
  return chord(root, quality);
}

// Tiny deterministic RNG: SplitMix64-style scrambling. We do NOT
// claim byte-identical output with ChaCha8Rng — see notes in the
// PR body. For the Random style we just need any deterministic
// seed → sequence mapping that is stable in TS.
function makeRng(seed: bigint): () => number {
  let state = seed;
  return () => {
    state = (state + 0x9e3779b97f4a7c15n) & 0xffffffffffffffffn;
    let z = state;
    z = (z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n;
    z = z & 0xffffffffffffffffn;
    z = (z ^ (z >> 27n)) * 0x94d049bb133111ebn;
    z = z & 0xffffffffffffffffn;
    z = z ^ (z >> 31n);
    // Convert to [0, 1)
    return Number(z & 0xffffffffn) / 0x100000000;
  };
}

export function generateProgression(
  key: Scale,
  lengthBars: number,
  style: ProgressionStyle,
  seed: number,
): Chord[] {
  if (lengthBars <= 0) return [];
  const cycle: Chord[] | null = (() => {
    if (style === "PopIVviIV" && key.mode === "Ionian") {
      return [diatonicChord(key, 1), diatonicChord(key, 5), diatonicChord(key, 6), diatonicChord(key, 4)];
    }
    if (style === "JazzIIVI" && key.mode === "Ionian") {
      return [diatonicChord(key, 2), diatonicChord(key, 5), diatonicChord(key, 1), diatonicChord(key, 6)];
    }
    if (style === "ModalDorian") {
      const dor: Scale = { root: key.root, mode: "Dorian" };
      return [
        diatonicChord(dor, 1),
        diatonicChord(dor, 4),
        diatonicChord(dor, 1),
        chord(pitchClassShift(dor.root, 10), "Maj"),
      ];
    }
    if (style === "BluesTwelveBar") {
      const i: Chord = chord(key.root, "Dom7");
      const iv: Chord = chord(pitchClassShift(key.root, 5), "Dom7");
      const v: Chord = chord(pitchClassShift(key.root, 7), "Dom7");
      return [i, i, i, i, iv, iv, i, i, v, iv, i, v];
    }
    return null;
  })();
  if (cycle && cycle.length > 0) {
    const out: Chord[] = [];
    for (let i = 0; i < lengthBars; i++) {
      out.push(cycle[i % cycle.length]);
    }
    return out;
  }
  // Random fallback
  const rng = makeRng(BigInt(seed));
  const out: Chord[] = [];
  for (let i = 0; i < lengthBars; i++) {
    const degree = 1 + Math.floor(rng() * 7); // 1..7
    out.push(diatonicChord(key, degree));
  }
  return out;
}

// -- Quantize -------------------------------------------------------------

function nearestPitchInPCs(p: Pitch, valid: PitchClass[]): Pitch | null {
  if (valid.length === 0) return null;
  if (valid.includes(pitchToPitchClass(p))) return p;
  for (let delta = 1; delta <= 6; delta++) {
    for (const sign of [1, -1]) {
      const candidate = p + sign * delta;
      if (candidate >= 0 && candidate <= 127) {
        const pc = ((candidate % 12) + 12) % 12;
        if (valid.includes(pc)) return candidate;
      }
    }
  }
  return null;
}

export function quantizeToScale(pitches: Pitch[], scale: Scale): Pitch[] {
  const valid = scalePitchClasses(scale);
  return pitches.map((p) => nearestPitchInPCs(p, valid) ?? p);
}

export function quantizeToChord(pitches: Pitch[], c: Chord): Pitch[] {
  const voicing = basicVoicing(c, 4);
  const valid = voicing.pitches.map(pitchToPitchClass);
  if (valid.length === 0) return pitches.slice();
  return pitches.map((p) => nearestPitchInPCs(p, valid) ?? p);
}

// -- Transforms -----------------------------------------------------------

export function transposeNotes(notes: Note[], semitones: Interval): Note[] {
  return notes.map((n) => ({ ...n, pitch: transposePitchClamped(n.pitch, semitones) }));
}

export function invertNotes(notes: Note[], axis: Pitch): Note[] {
  return notes.map((n) => ({ ...n, pitch: clampPitch(2 * axis - n.pitch) }));
}

export function retrogradeNotes(notes: Note[]): Note[] {
  if (notes.length === 0) return [];
  const span = notes.reduce((m, n) => Math.max(m, n.start_ticks + n.length_ticks), 0);
  return notes
    .map((n) => ({ ...n, start_ticks: Math.max(0, span - (n.start_ticks + n.length_ticks)) }))
    .sort((a, b) => a.start_ticks - b.start_ticks);
}

export function augmentNotes(notes: Note[], factor: number): Note[] {
  if (!Number.isFinite(factor) || factor <= 0) return notes.slice();
  return notes.map((n) => ({
    ...n,
    start_ticks: Math.max(0, Math.round(n.start_ticks * factor)),
    length_ticks: Math.max(1, Math.round(n.length_ticks * factor)),
  }));
}

export function humanizeNotes(
  notes: Note[],
  velocityJitter: number,
  timingJitterTicks: number,
  seed: number,
): Note[] {
  const rng = makeRng(BigInt(seed));
  return notes.map((n) => {
    let velocity = n.velocity;
    let start = n.start_ticks;
    if (velocityJitter > 0) {
      const delta = Math.round((rng() * 2 - 1) * velocityJitter);
      velocity = Math.max(0, Math.min(127, n.velocity + delta));
    }
    if (timingJitterTicks > 0) {
      const delta = Math.round((rng() * 2 - 1) * timingJitterTicks);
      start = Math.max(0, n.start_ticks + delta);
    }
    return { ...n, velocity, start_ticks: start };
  });
}
