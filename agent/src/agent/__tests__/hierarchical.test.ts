import { describe, expect, test } from "bun:test";

import type { FormPlan, SectionOutline, SectionPlan } from "../form";
import { HierarchicalPlanner, type ComposerCaller, type DirectorCaller } from "../hierarchical";
import type { ToolCall } from "../safety";

function sectionOutline(
  role: SectionOutline["role"],
  lengthBars: number,
  targetEnergy = 0.5,
): SectionOutline {
  return {
    name: role,
    role,
    lengthBars,
    preferredPatternIds: [],
    targetEnergy,
  };
}

function fixedDirector(form: FormPlan): DirectorCaller {
  return { async plan() { return form; } };
}

function callCountingComposer(plans: SectionPlan[]): { caller: ComposerCaller; calls: number[] } {
  const calls: number[] = [];
  return {
    calls,
    caller: {
      async plan({ sectionIndex }) {
        calls.push(sectionIndex);
        return plans[sectionIndex];
      },
    },
  };
}

describe("HierarchicalPlanner", () => {
  test("startSong stores the Director form", async () => {
    const form: FormPlan = {
      brief: "lo-fi 30min",
      totalBars: 32,
      sections: [sectionOutline("intro", 4), sectionOutline("verse", 12), sectionOutline("chorus", 16)],
      climaxBarIndex: 28,
    };
    const planner = new HierarchicalPlanner(fixedDirector(form), {
      async plan() { throw new Error("unused"); },
    });
    expect(planner.hasForm()).toBe(false);
    const out = await planner.startSong("anything");
    expect(out).toEqual(form);
    expect(planner.hasForm()).toBe(true);
  });

  test("sectionIndexForBar walks across section boundaries", async () => {
    const form: FormPlan = {
      brief: "x",
      totalBars: 32,
      sections: [sectionOutline("intro", 4), sectionOutline("verse", 12), sectionOutline("chorus", 16)],
      climaxBarIndex: 28,
    };
    const planner = new HierarchicalPlanner(fixedDirector(form), {
      async plan() { throw new Error(""); },
    });
    await planner.startSong("");
    expect(planner.sectionIndexForBar(0)).toBe(0);
    expect(planner.sectionIndexForBar(3)).toBe(0);
    expect(planner.sectionIndexForBar(4)).toBe(1);
    expect(planner.sectionIndexForBar(15)).toBe(1);
    expect(planner.sectionIndexForBar(16)).toBe(2);
    expect(planner.sectionIndexForBar(31)).toBe(2);
    expect(planner.sectionIndexForBar(32)).toBe(-1);
  });

  test("sectionPlan caches per-section so Composer is called once", async () => {
    const form: FormPlan = {
      brief: "x",
      totalBars: 8,
      sections: [sectionOutline("intro", 4), sectionOutline("verse", 4)],
      climaxBarIndex: 6,
    };
    const setupCalls0: ToolCall[] = [{ name: "set_transport", args: { bpm: 120 } }];
    const setupCalls1: ToolCall[] = [{ name: "set_energy", args: { targetEnergy: 0.7 } }];
    const sectionPlans: SectionPlan[] = [
      { sectionIndex: 0, summary: "intro pad", setupCalls: setupCalls0 },
      { sectionIndex: 1, summary: "verse", setupCalls: setupCalls1 },
    ];
    const { caller, calls } = callCountingComposer(sectionPlans);
    const planner = new HierarchicalPlanner(fixedDirector(form), caller);
    await planner.startSong("");

    const p1 = await planner.sectionPlan(0);
    const p2 = await planner.sectionPlan(0);
    expect(p1).toEqual(p2);
    expect(calls).toEqual([0]);

    await planner.sectionPlan(1);
    expect(calls).toEqual([0, 1]);
  });

  test("invalidateSectionPlan forces re-composition", async () => {
    const form: FormPlan = {
      brief: "x",
      totalBars: 4,
      sections: [sectionOutline("intro", 4)],
      climaxBarIndex: 0,
    };
    const { caller, calls } = callCountingComposer([
      { sectionIndex: 0, summary: "first", setupCalls: [] },
    ]);
    const planner = new HierarchicalPlanner(fixedDirector(form), caller);
    await planner.startSong("");
    await planner.sectionPlan(0);
    planner.invalidateSectionPlan(0);
    await planner.sectionPlan(0);
    expect(calls).toEqual([0, 0]);
  });

  test("setupCallsForRange concatenates per-section setup calls in order", async () => {
    const form: FormPlan = {
      brief: "x",
      totalBars: 12,
      sections: [
        sectionOutline("intro", 4),
        sectionOutline("verse", 4),
        sectionOutline("chorus", 4),
      ],
      climaxBarIndex: 9,
    };
    const { caller } = callCountingComposer([
      { sectionIndex: 0, summary: "a", setupCalls: [{ name: "a", args: {} }] },
      { sectionIndex: 1, summary: "b", setupCalls: [{ name: "b", args: {} }] },
      { sectionIndex: 2, summary: "c", setupCalls: [{ name: "c", args: {} }] },
    ]);
    const planner = new HierarchicalPlanner(fixedDirector(form), caller);
    await planner.startSong("");
    const all = await planner.setupCallsForRange(0, 2);
    expect(all.map((c) => c.name)).toEqual(["a", "b", "c"]);
  });

  test("sectionPlan errors when called before startSong", async () => {
    const planner = new HierarchicalPlanner(
      { async plan() { throw new Error("not called"); } },
      { async plan() { throw new Error("not called"); } },
    );
    await expect(planner.sectionPlan(0)).rejects.toThrow(/startSong/);
  });

  test("sectionPlan errors on out-of-range index", async () => {
    const form: FormPlan = {
      brief: "x",
      totalBars: 4,
      sections: [sectionOutline("intro", 4)],
      climaxBarIndex: 0,
    };
    const planner = new HierarchicalPlanner(fixedDirector(form), {
      async plan() { throw new Error(""); },
    });
    await planner.startSong("");
    await expect(planner.sectionPlan(5)).rejects.toThrow(/out of range/);
  });
});
