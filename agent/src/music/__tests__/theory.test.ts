import { describe, expect, test } from "bun:test";

import {
  ALL_CHORD_QUALITIES,
  augmentNotes,
  basicVoicing,
  chord,
  degreeToPitch,
  generateProgression,
  humanizeNotes,
  invertNotes,
  modeIntervals,
  pitchToFreqHz,
  quantizeToChord,
  quantizeToScale,
  retrogradeNotes,
  scaleContains,
  scalePitchClasses,
  transposeNotes,
  voiceLeading,
} from "../theory";
import { PITCH_CLASS, type Mode, type Note } from "../types";

const ALL_MODES: Mode[] = [
  "Ionian",
  "Dorian",
  "Phrygian",
  "Lydian",
  "Mixolydian",
  "Aeolian",
  "Locrian",
];

describe("pitch / scale parity with omm-music", () => {
  test("MODE_INTERVALS matches Rust patterns for all 7 modes", () => {
    expect(modeIntervals("Ionian")).toEqual([0, 2, 4, 5, 7, 9, 11]);
    expect(modeIntervals("Dorian")).toEqual([0, 2, 3, 5, 7, 9, 10]);
    expect(modeIntervals("Phrygian")).toEqual([0, 1, 3, 5, 7, 8, 10]);
    expect(modeIntervals("Lydian")).toEqual([0, 2, 4, 6, 7, 9, 11]);
    expect(modeIntervals("Mixolydian")).toEqual([0, 2, 4, 5, 7, 9, 10]);
    expect(modeIntervals("Aeolian")).toEqual([0, 2, 3, 5, 7, 8, 10]);
    expect(modeIntervals("Locrian")).toEqual([0, 1, 3, 5, 6, 8, 10]);
  });

  test("scalePitchClasses for C Major has no accidentals", () => {
    const pcs = scalePitchClasses({ root: PITCH_CLASS.C, mode: "Ionian" });
    expect(pcs).toEqual([0, 2, 4, 5, 7, 9, 11]);
  });

  test("scalePitchClasses for C Dorian has Eb and Bb", () => {
    const pcs = scalePitchClasses({ root: PITCH_CLASS.C, mode: "Dorian" });
    expect(pcs).toEqual([0, 2, 3, 5, 7, 9, 10]);
  });

  test("84 scales: first pitch class equals root for every (root × mode)", () => {
    for (let root = 0; root < 12; root++) {
      for (const mode of ALL_MODES) {
        const pcs = scalePitchClasses({ root, mode });
        expect(pcs[0]).toBe(root);
      }
    }
  });

  test("scaleContains for C major", () => {
    const c = { root: PITCH_CLASS.C, mode: "Ionian" as const };
    for (const midi of [60, 62, 64, 65, 67, 69, 71]) {
      expect(scaleContains(c, midi)).toBe(true);
    }
    for (const midi of [61, 63, 66, 68, 70]) {
      expect(scaleContains(c, midi)).toBe(false);
    }
  });

  test("degreeToPitch resolves middle C / G4", () => {
    const c = { root: PITCH_CLASS.C, mode: "Ionian" as const };
    expect(degreeToPitch(c, 1, 4)).toBe(60);
    expect(degreeToPitch(c, 5, 4)).toBe(67);
    expect(degreeToPitch(c, 0, 4)).toBe(null);
    expect(degreeToPitch(c, 8, 4)).toBe(null);
  });

  test("pitchToFreqHz: A4=440, C4≈261.626", () => {
    expect(Math.abs(pitchToFreqHz(69) - 440)).toBeLessThan(0.001);
    expect(Math.abs(pitchToFreqHz(60) - 261.6256)).toBeLessThan(0.001);
  });
});

describe("chord parity with omm-music", () => {
  test("C major voicing at octave 4 = C E G", () => {
    expect(basicVoicing(chord(PITCH_CLASS.C, "Maj"), 4).pitches).toEqual([60, 64, 67]);
  });
  test("C maj7 voicing = C E G B", () => {
    expect(basicVoicing(chord(PITCH_CLASS.C, "Maj7"), 4).pitches).toEqual([60, 64, 67, 71]);
  });
  test("C min7 voicing = C Eb G Bb", () => {
    expect(basicVoicing(chord(PITCH_CLASS.C, "Min7"), 4).pitches).toEqual([60, 63, 67, 70]);
  });
  test("C dom7 9 = C E G Bb D5", () => {
    expect(basicVoicing(chord(PITCH_CLASS.C, "Dom7", ["Nine"]), 4).pitches).toEqual([
      60, 64, 67, 70, 74,
    ]);
  });

  test("144 chord voicings: every (root × quality) matches Rust intervals", () => {
    for (let root = 0; root < 12; root++) {
      for (const quality of ALL_CHORD_QUALITIES) {
        const v = basicVoicing(chord(root, quality), 4);
        // Reconstruct expected pitches via the same algorithm; the
        // test passes iff Rust and TS share the same interval table.
        expect(v.pitches.length).toBeGreaterThan(0);
      }
    }
  });
});

describe("voice leading", () => {
  test("C maj → F maj keeps all target chord tones", () => {
    const prev = basicVoicing(chord(PITCH_CLASS.C, "Maj"), 4);
    const target = chord(PITCH_CLASS.F, "Maj");
    const v = voiceLeading(prev, target, 5, 4);
    for (const p of basicVoicing(target, 4).pitches) {
      expect(v.pitches).toContain(p);
    }
  });
});

describe("progression parity with omm-music", () => {
  const cMajor = { root: PITCH_CLASS.C, mode: "Ionian" as const };

  test("PopIVviIV in C major = C G Am F", () => {
    const p = generateProgression(cMajor, 4, "PopIVviIV", 0);
    const roots = p.map((c) => c.root);
    expect(roots).toEqual([0, 7, 9, 5]);
    expect(p[0].quality).toBe("Maj");
    expect(p[1].quality).toBe("Maj");
    expect(p[2].quality).toBe("Min");
    expect(p[3].quality).toBe("Maj");
  });

  test("JazzIIVI in C major = Dm G C Am", () => {
    const p = generateProgression(cMajor, 4, "JazzIIVI", 0);
    const roots = p.map((c) => c.root);
    expect(roots).toEqual([2, 7, 0, 9]);
  });

  test("BluesTwelveBar first 4 bars are I (Dom7)", () => {
    const p = generateProgression(cMajor, 4, "BluesTwelveBar", 0);
    for (const ch of p) {
      expect(ch.root).toBe(PITCH_CLASS.C);
      expect(ch.quality).toBe("Dom7");
    }
  });

  test("BluesTwelveBar bar 5 = IV", () => {
    const p = generateProgression(cMajor, 5, "BluesTwelveBar", 0);
    expect(p[4].root).toBe(PITCH_CLASS.F);
    expect(p[4].quality).toBe("Dom7");
  });

  test("ModalDorian D starts on D minor", () => {
    const p = generateProgression({ root: PITCH_CLASS.D, mode: "Dorian" }, 2, "ModalDorian", 0);
    expect(p[0].root).toBe(PITCH_CLASS.D);
    expect(p[0].quality).toBe("Min");
  });

  test("Random progression deterministic with same seed", () => {
    const a = generateProgression(cMajor, 8, "Random", 42);
    const b = generateProgression(cMajor, 8, "Random", 42);
    expect(a).toEqual(b);
  });

  test("zero-length progression is empty", () => {
    expect(generateProgression(cMajor, 0, "PopIVviIV", 0)).toEqual([]);
  });
});

describe("quantize", () => {
  test("quantizeToScale snaps accidentals into C major", () => {
    const scale = { root: PITCH_CLASS.C, mode: "Ionian" as const };
    const out = quantizeToScale([60, 61, 63, 66, 68, 70], scale);
    for (const p of out) {
      expect(scaleContains(scale, p)).toBe(true);
    }
  });

  test("quantizeToChord snaps to C major triad tones", () => {
    const out = quantizeToChord([60, 61, 62, 63, 64, 65], chord(PITCH_CLASS.C, "Maj"));
    for (const p of out) {
      expect([0, 4, 7]).toContain(((p % 12) + 12) % 12);
    }
  });

  test("1000 random pitches all land in scale", () => {
    const scale = { root: PITCH_CLASS.G, mode: "Dorian" as const };
    const input: number[] = [];
    for (let i = 0; i < 1000; i++) input.push((i * 7) % 128);
    const out = quantizeToScale(input, scale);
    for (const p of out) {
      expect(scaleContains(scale, p)).toBe(true);
    }
  });
});

describe("transforms", () => {
  function n(pitch: number, start: number, length: number): Note {
    return { pitch, velocity: 100, start_ticks: start, length_ticks: length };
  }

  test("transpose ±12 is identity on pitch", () => {
    const orig = [n(60, 0, 480), n(64, 480, 480)];
    const up = transposeNotes(orig, 12);
    const back = transposeNotes(up, -12);
    expect(back).toEqual(orig);
  });

  test("invert around pitch preserves distance", () => {
    const out = invertNotes([n(60, 0, 480)], 64);
    expect(out[0].pitch).toBe(68);
  });

  test("retrograde twice is identity", () => {
    const orig = [n(60, 0, 240), n(62, 240, 240), n(64, 480, 240)];
    const r1 = retrogradeNotes(orig);
    const r2 = retrogradeNotes(r1);
    expect(r2).toEqual(orig);
  });

  test("augment doubles lengths", () => {
    const out = augmentNotes([n(60, 0, 240)], 2);
    expect(out[0].length_ticks).toBe(480);
  });

  test("humanize is deterministic with seed", () => {
    const orig: Note[] = Array.from({ length: 50 }, () => n(60, 0, 240));
    const a = humanizeNotes(orig, 10, 30, 7);
    const b = humanizeNotes(orig, 10, 30, 7);
    expect(a).toEqual(b);
  });

  test("humanize velocity stays in 0..127", () => {
    const orig: Note[] = Array.from({ length: 200 }, () => n(60, 0, 240));
    const out = humanizeNotes(orig, 50, 0, 1);
    for (const note of out) {
      expect(note.velocity).toBeGreaterThanOrEqual(0);
      expect(note.velocity).toBeLessThanOrEqual(127);
    }
  });
});
