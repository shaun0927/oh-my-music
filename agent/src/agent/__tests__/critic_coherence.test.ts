import { describe, expect, test } from "bun:test";

import { CriticOrchestrator, type CriticCaller, type RenderObservation } from "../critic";
import { MotifRegistry, TensionTracker, rmsDbToEnergy } from "../coherence";
import type { FormPlan } from "../form";

function fixedCritic(verdict: "accept" | "revise" | "reject", note = ""): CriticCaller {
  return {
    async critique({ observation }) {
      return {
        sectionIndex: observation.sectionIndex,
        verdict,
        note,
        suggestedFollowUps: [],
      };
    },
  };
}

function obs(idx: number, passed = true): RenderObservation {
  return {
    sectionIndex: idx,
    peak: 0.5,
    rms: 0.3,
    dynamicRangeDb: 8,
    clippedSampleRatio: 0,
    silenceRatio: 0.1,
    estimatedOnsetRatePerSec: 2,
    deterministicPassed: passed,
  };
}

describe("CriticOrchestrator", () => {
  test("observe stores per-section feedback in arrival order", async () => {
    const c = new CriticOrchestrator(fixedCritic("accept", "ok"));
    await c.observe("lo-fi", obs(0));
    await c.observe("lo-fi", obs(0));
    expect(c.feedbackFor(0)).toHaveLength(2);
  });

  test("shouldCommit returns false if deterministic fails", async () => {
    const c = new CriticOrchestrator(fixedCritic("accept"));
    await c.observe("x", obs(0));
    expect(c.shouldCommit(0, false)).toBe(false);
  });

  test("shouldCommit returns true if Critic accepts and deterministic passes", async () => {
    const c = new CriticOrchestrator(fixedCritic("accept"));
    await c.observe("x", obs(0));
    expect(c.shouldCommit(0, true)).toBe(true);
  });

  test("shouldCommit returns false when Critic says revise", async () => {
    const c = new CriticOrchestrator(fixedCritic("revise", "too dense"));
    await c.observe("x", obs(0));
    expect(c.shouldCommit(0, true)).toBe(false);
  });

  test("shouldCommit returns true with no Critic feedback yet (default ok)", () => {
    const c = new CriticOrchestrator(fixedCritic("accept"));
    expect(c.shouldCommit(99, true)).toBe(true);
  });

  test("latestFeedback returns most recent entry", async () => {
    const c = new CriticOrchestrator(fixedCritic("accept", "first"));
    await c.observe("x", obs(0));
    // Swap caller to return "revise" — simulate Composer re-rendering
    // and Critic disliking the second take.
    const c2 = new CriticOrchestrator(fixedCritic("revise", "second"));
    await c2.observe("x", obs(0));
    expect(c2.latestFeedback(0)!.note).toBe("second");
  });
});

describe("MotifRegistry", () => {
  test("planned callback starts as not heard", () => {
    const r = new MotifRegistry();
    r.planCallback({
      patternId: "p1",
      originSectionIndex: 0,
      recurrenceSectionIndex: 3,
      transformation: "transpose -3",
    });
    expect(r.all()).toHaveLength(1);
    expect(r.all()[0].heard).toBe(false);
  });

  test("markHeard flips callbacks whose recurrence matches", () => {
    const r = new MotifRegistry();
    r.planCallback({
      patternId: "p1",
      originSectionIndex: 0,
      recurrenceSectionIndex: 3,
      transformation: "transpose -3",
    });
    r.planCallback({
      patternId: "p2",
      originSectionIndex: 1,
      recurrenceSectionIndex: 4,
      transformation: "retrograde",
    });
    const heard = r.markHeard(3);
    expect(heard).toHaveLength(1);
    expect(heard[0].patternId).toBe("p1");
  });

  test("missed returns callbacks past their recurrence", () => {
    const r = new MotifRegistry();
    r.planCallback({
      patternId: "p1",
      originSectionIndex: 0,
      recurrenceSectionIndex: 2,
      transformation: "x",
    });
    expect(r.missed(3)).toHaveLength(1);
    r.markHeard(2);
    expect(r.missed(3)).toHaveLength(0);
  });
});

describe("TensionTracker", () => {
  const form: FormPlan = {
    brief: "x",
    totalBars: 8,
    sections: [
      { name: "intro", role: "intro", lengthBars: 4, preferredPatternIds: [], targetEnergy: 0.3 },
      { name: "verse", role: "verse", lengthBars: 4, preferredPatternIds: [], targetEnergy: 0.7 },
    ],
    climaxBarIndex: 6,
  };

  test("maxDrift reports largest planned vs actual delta", () => {
    const t = new TensionTracker();
    t.recordSection(0, 0.3, 0.4);
    t.recordSection(1, 0.7, 0.2);
    expect(t.maxDrift()).toBeCloseTo(0.5);
  });

  test("summary mentions last section and drift", () => {
    const t = new TensionTracker();
    t.recordSection(0, 0.3, 0.4);
    t.recordSection(1, 0.7, 0.6);
    const s = t.summary(form);
    expect(s).toContain("verse");
    expect(s).toContain("maxDrift");
  });

  test("summary handles empty state", () => {
    const t = new TensionTracker();
    expect(t.summary(form)).toBe("no samples yet");
  });
});

describe("rmsDbToEnergy", () => {
  test("clamps below -40 to 0 and above -6 to 1", () => {
    expect(rmsDbToEnergy(-60)).toBe(0);
    expect(rmsDbToEnergy(0)).toBe(1);
  });

  test("maps midpoint -23 dB to ~0.5", () => {
    expect(rmsDbToEnergy(-23)).toBeCloseTo(0.5, 1);
  });
});
