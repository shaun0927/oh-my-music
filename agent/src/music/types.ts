// TypeScript mirror of `crates/omm-music` core types. Algorithms in
// `./theory.ts` MUST produce the same output as the Rust crate for
// every deterministic function — the inline golden tests pin that.

export type Pitch = number; // MIDI 0..=127
export type PitchClass = number; // 0..=11
export type Interval = number; // semitones (signed)

export const PITCH_CLASS = {
  C: 0,
  C_SHARP: 1,
  D: 2,
  D_SHARP: 3,
  E: 4,
  F: 5,
  F_SHARP: 6,
  G: 7,
  G_SHARP: 8,
  A: 9,
  A_SHARP: 10,
  B: 11,
} as const;

export type Mode =
  | "Ionian"
  | "Dorian"
  | "Phrygian"
  | "Lydian"
  | "Mixolydian"
  | "Aeolian"
  | "Locrian";

export const MODE_ALIAS = {
  Major: "Ionian" as const,
  Minor: "Aeolian" as const,
};

export interface Scale {
  root: PitchClass;
  mode: Mode;
}

export type ChordQuality =
  | "Maj"
  | "Min"
  | "Dim"
  | "Aug"
  | "Sus2"
  | "Sus4"
  | "Dom7"
  | "Maj7"
  | "Min7"
  | "Min7b5"
  | "Dim7"
  | "MinMaj7";

export type Extension =
  | "Nine"
  | "FlatNine"
  | "SharpNine"
  | "Eleven"
  | "SharpEleven"
  | "FlatThirteen"
  | "Thirteen";

export interface Chord {
  root: PitchClass;
  quality: ChordQuality;
  extensions: Extension[];
}

export interface Voicing {
  pitches: Pitch[];
}

export interface Note {
  pitch: Pitch;
  velocity: number; // 0..=127
  start_ticks: number;
  length_ticks: number;
}

export type ProgressionStyle =
  | "PopIVviIV"
  | "JazzIIVI"
  | "ModalDorian"
  | "BluesTwelveBar"
  | "Random";
