// LLM tools backed by the deterministic music-theory port in
// `agent/src/music/theory.ts`. These let the LLM pick from valid
// chord/scale/voicing options instead of guessing intervals.

import { Type } from "@mariozechner/pi-ai";
import { defineTool } from "@mariozechner/pi-coding-agent";

import {
  basicVoicing,
  chord as makeChord,
  generateProgression,
  humanizeNotes,
  invertNotes,
  quantizeToScale,
  retrogradeNotes,
  transposeNotes,
  voiceLeading,
} from "../../music/theory";
import type { Chord, Note, Pitch } from "../../music/types";

const MODE_LITERAL = Type.Union([
  Type.Literal("Ionian"),
  Type.Literal("Dorian"),
  Type.Literal("Phrygian"),
  Type.Literal("Lydian"),
  Type.Literal("Mixolydian"),
  Type.Literal("Aeolian"),
  Type.Literal("Locrian"),
]);

const STYLE_LITERAL = Type.Union([
  Type.Literal("PopIVviIV"),
  Type.Literal("JazzIIVI"),
  Type.Literal("ModalDorian"),
  Type.Literal("BluesTwelveBar"),
  Type.Literal("Random"),
]);

const NOTE_SCHEMA = Type.Object({
  pitch: Type.Number({ minimum: 0, maximum: 127 }),
  velocity: Type.Number({ minimum: 0, maximum: 127 }),
  start_ticks: Type.Number({ minimum: 0 }),
  length_ticks: Type.Number({ minimum: 1 }),
});

function pitchOk(n: number): n is Pitch {
  return Number.isInteger(n) && n >= 0 && n <= 127;
}

export function generateProgressionTool() {
  return defineTool({
    name: "generate_progression",
    label: "Generate Chord Progression",
    description:
      "Deterministic chord progression generator. Pick from 4 canonical styles or a seeded random walk.",
    parameters: Type.Object({
      keyRoot: Type.Number({ minimum: 0, maximum: 11 }),
      mode: MODE_LITERAL,
      lengthBars: Type.Number({ minimum: 1, maximum: 64 }),
      style: STYLE_LITERAL,
      seed: Type.Number({ minimum: 0 }),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const chords = generateProgression(
        { root: params.keyRoot, mode: params.mode },
        params.lengthBars,
        params.style,
        params.seed,
      );
      return {
        content: [
          {
            type: "text",
            text: `Generated ${chords.length} chord(s) (${params.style} in mode ${params.mode}).`,
          },
        ],
        details: { chords },
      };
    },
  });
}

export function voiceLeadTool() {
  return defineTool({
    name: "voice_lead",
    label: "Voice Lead To Chord",
    description:
      "Greedy voice-leading from prev voicing to the next chord, snapping each voice to the nearest target chord tone within a movement cap.",
    parameters: Type.Object({
      prevPitches: Type.Array(Type.Number({ minimum: 0, maximum: 127 })),
      target: Type.Object({
        root: Type.Number({ minimum: 0, maximum: 11 }),
        quality: Type.String(),
        extensions: Type.Optional(Type.Array(Type.String())),
      }),
      maxMovementSemitones: Type.Number({ minimum: 0, maximum: 24 }),
      targetRootOctave: Type.Number({ minimum: -1, maximum: 9 }),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const prev = { pitches: params.prevPitches.filter(pitchOk) };
      const target: Chord = {
        root: params.target.root,
        quality: params.target.quality as Chord["quality"],
        extensions: (params.target.extensions ?? []) as Chord["extensions"],
      };
      const v = voiceLeading(prev, target, params.maxMovementSemitones, params.targetRootOctave);
      return {
        content: [
          {
            type: "text",
            text: `Voiced to ${v.pitches.length} pitches.`,
          },
        ],
        details: { voicing: v.pitches },
      };
    },
  });
}

export function basicVoicingTool() {
  return defineTool({
    name: "basic_voicing",
    label: "Build Basic Voicing",
    description: "Construct the root-position voicing for a chord at the given octave.",
    parameters: Type.Object({
      root: Type.Number({ minimum: 0, maximum: 11 }),
      quality: Type.String(),
      extensions: Type.Optional(Type.Array(Type.String())),
      rootOctave: Type.Number({ minimum: -1, maximum: 9 }),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const c = makeChord(
        params.root,
        params.quality as Chord["quality"],
        (params.extensions ?? []) as Chord["extensions"],
      );
      const v = basicVoicing(c, params.rootOctave);
      return {
        content: [{ type: "text", text: `Voicing: ${v.pitches.join(", ")}` }],
        details: { voicing: v.pitches },
      };
    },
  });
}

export function quantizeToScaleTool() {
  return defineTool({
    name: "quantize_to_scale",
    label: "Quantize To Scale",
    description: "Snap every pitch to the nearest member of the given scale.",
    parameters: Type.Object({
      pitches: Type.Array(Type.Number({ minimum: 0, maximum: 127 })),
      keyRoot: Type.Number({ minimum: 0, maximum: 11 }),
      mode: MODE_LITERAL,
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const out = quantizeToScale(params.pitches.filter(pitchOk), {
        root: params.keyRoot,
        mode: params.mode,
      });
      return {
        content: [{ type: "text", text: `Quantized ${out.length} pitches.` }],
        details: { pitches: out },
      };
    },
  });
}

export function transposeMotifTool() {
  return defineTool({
    name: "transpose_motif",
    label: "Transpose Motif",
    description: "Transpose every note by the given interval in semitones (clamped to MIDI range).",
    parameters: Type.Object({
      notes: Type.Array(NOTE_SCHEMA),
      byInterval: Type.Number({ minimum: -127, maximum: 127 }),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const out = transposeNotes(params.notes as Note[], params.byInterval);
      return {
        content: [{ type: "text", text: `Transposed ${out.length} notes.` }],
        details: { notes: out },
      };
    },
  });
}

export function invertMotifTool() {
  return defineTool({
    name: "invert_motif",
    label: "Invert Motif",
    description: "Invert every pitch around the axis (pitch -> 2*axis - pitch).",
    parameters: Type.Object({
      notes: Type.Array(NOTE_SCHEMA),
      aroundPitch: Type.Number({ minimum: 0, maximum: 127 }),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const out = invertNotes(params.notes as Note[], params.aroundPitch);
      return {
        content: [{ type: "text", text: `Inverted ${out.length} notes around ${params.aroundPitch}.` }],
        details: { notes: out },
      };
    },
  });
}

export function retrogradeMotifTool() {
  return defineTool({
    name: "retrograde_motif",
    label: "Retrograde Motif",
    description: "Reverse the temporal order of the notes (preserves note lengths).",
    parameters: Type.Object({
      notes: Type.Array(NOTE_SCHEMA),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const out = retrogradeNotes(params.notes as Note[]);
      return {
        content: [{ type: "text", text: `Retrograded ${out.length} notes.` }],
        details: { notes: out },
      };
    },
  });
}

export function humanizeMotifTool() {
  return defineTool({
    name: "humanize_motif",
    label: "Humanize Motif",
    description:
      "Add deterministic bounded jitter to velocity and start position. Same seed → same output.",
    parameters: Type.Object({
      notes: Type.Array(NOTE_SCHEMA),
      velocityJitter: Type.Number({ minimum: 0, maximum: 64 }),
      timingJitterTicks: Type.Number({ minimum: 0, maximum: 480 }),
      seed: Type.Number({ minimum: 0 }),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const out = humanizeNotes(
        params.notes as Note[],
        params.velocityJitter,
        params.timingJitterTicks,
        params.seed,
      );
      return {
        content: [{ type: "text", text: `Humanized ${out.length} notes.` }],
        details: { notes: out },
      };
    },
  });
}

/// Convenience: collect every theory tool the agent should expose.
export function theoryTools() {
  return [
    generateProgressionTool(),
    voiceLeadTool(),
    basicVoicingTool(),
    quantizeToScaleTool(),
    transposeMotifTool(),
    invertMotifTool(),
    retrogradeMotifTool(),
    humanizeMotifTool(),
  ];
}
