import { describe, expect, test } from "bun:test";

import { PatternStore, type Pattern } from "../store";
import { varyPattern } from "../variation";

function note(pitch: number, start: number, length: number, velocity = 100) {
  return { pitch, velocity, start_ticks: start, length_ticks: length };
}

function samplePattern(id: string, notes = [note(60, 0, 480), note(64, 480, 480)]): Pattern {
  return {
    id,
    name: id,
    role: "motif",
    notes,
    lengthBars: 1,
    tags: ["test"],
    createdAt: new Date().toISOString(),
  };
}

describe("PatternStore", () => {
  test("save → recall round-trips a pattern", () => {
    const s = new PatternStore();
    s.save(samplePattern("p1"));
    const back = s.recall("p1");
    expect(back).not.toBeNull();
    expect(back!.id).toBe("p1");
    expect(back!.notes).toHaveLength(2);
  });

  test("list filters by role and tag", () => {
    const s = new PatternStore();
    s.save({ ...samplePattern("p1"), role: "verse", tags: ["pop"] });
    s.save({ ...samplePattern("p2"), role: "chorus", tags: ["pop"] });
    s.save({ ...samplePattern("p3"), role: "chorus", tags: ["jazz"] });
    expect(s.list({ role: "chorus" })).toHaveLength(2);
    expect(s.list({ tag: "pop" })).toHaveLength(2);
    expect(s.list({ role: "chorus", tag: "pop" })).toHaveLength(1);
  });

  test("remove deletes by id", () => {
    const s = new PatternStore();
    s.save(samplePattern("p1"));
    expect(s.remove("p1")).toBe(true);
    expect(s.recall("p1")).toBeNull();
    expect(s.remove("p1")).toBe(false);
  });

  test("save with same id overwrites (UPSERT)", () => {
    const s = new PatternStore();
    s.save(samplePattern("p1"));
    s.save({ ...samplePattern("p1"), name: "renamed" });
    expect(s.recall("p1")!.name).toBe("renamed");
  });

  test("summary returns lightweight view", () => {
    const s = new PatternStore();
    s.save(samplePattern("p1"));
    s.save(samplePattern("p2"));
    const view = s.summary();
    expect(view).toHaveLength(2);
    expect(view[0]).toHaveProperty("id");
    expect(view[0]).toHaveProperty("lengthBars");
    expect((view[0] as any).notes).toBeUndefined(); // exclude notes
  });

  test("count tracks inserts and deletes", () => {
    const s = new PatternStore();
    expect(s.count()).toBe(0);
    s.save(samplePattern("p1"));
    s.save(samplePattern("p2"));
    expect(s.count()).toBe(2);
    s.remove("p1");
    expect(s.count()).toBe(1);
  });
});

describe("varyPattern", () => {
  test("transpose +12 shifts every note up an octave", () => {
    const source = samplePattern("orig", [note(60, 0, 480), note(64, 480, 480)]);
    const v = varyPattern(source, { kind: "transpose", intervalSemitones: 12 }, "orig-up");
    expect(v.notes.map((n) => n.pitch)).toEqual([72, 76]);
    expect(v.tags).toContain("variation");
  });

  test("retrograde reverses order", () => {
    const source = samplePattern("orig", [
      note(60, 0, 240),
      note(62, 240, 240),
      note(64, 480, 240),
    ]);
    const v = varyPattern(source, { kind: "retrograde" }, "orig-rev");
    expect(v.notes.map((n) => n.pitch)).toEqual([64, 62, 60]);
  });

  test("composite chains multiple steps deterministically", () => {
    const source = samplePattern("orig");
    const v = varyPattern(
      source,
      {
        kind: "composite",
        steps: [
          { kind: "transpose", intervalSemitones: 5 },
          { kind: "humanize", velocityJitter: 0, timingJitterTicks: 0, seed: 1 },
        ],
      },
      "orig-composite",
    );
    expect(v.notes[0].pitch).toBe(65);
  });

  test("humanize is reproducible with same seed", () => {
    const source = samplePattern("orig");
    const a = varyPattern(
      source,
      { kind: "humanize", velocityJitter: 10, timingJitterTicks: 20, seed: 42 },
      "a",
    );
    const b = varyPattern(
      source,
      { kind: "humanize", velocityJitter: 10, timingJitterTicks: 20, seed: 42 },
      "b",
    );
    expect(a.notes).toEqual(b.notes);
  });

  test("variation preserves role and lengthBars from source", () => {
    const source = { ...samplePattern("orig"), role: "chorus" as const, lengthBars: 4 };
    const v = varyPattern(source, { kind: "retrograde" }, "v");
    expect(v.role).toBe("chorus");
    expect(v.lengthBars).toBe(4);
  });
});
