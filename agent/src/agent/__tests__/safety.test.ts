import { describe, expect, test } from "bun:test";

import { SafetyPolicy, type ToolCall, DEFAULT_SAFETY_CONFIG } from "../safety";

function call(name: string, args: Record<string, unknown> = {}): ToolCall {
  return { name, args };
}

describe("SafetyPolicy", () => {
  test("accepts a single safe tool call", () => {
    const sp = new SafetyPolicy();
    const verdict = sp.evaluateCycle([call("set_energy", { targetEnergy: 0.5 })], 0);
    expect(verdict.accepted).toHaveLength(1);
    expect(verdict.rejected).toHaveLength(0);
  });

  test("rejects master gain above ceiling", () => {
    const sp = new SafetyPolicy();
    const verdict = sp.evaluateCycle([call("set_master_gain_db", { gain_db: 3 })], 0);
    expect(verdict.accepted).toHaveLength(0);
    expect(verdict.rejected).toHaveLength(1);
    expect(verdict.rejected[0].reason).toMatch(/ceiling/);
  });

  test("rejects per-cycle gain increase exceeding cap", () => {
    const sp = new SafetyPolicy({
      maxGainIncreasePerCycleDb: 3,
      masterGainCeilingDb: 6, // ceiling raised so the test isolates the per-cycle cap
      maxToolCallsPerCycle: 8,
      glicolCooldownMs: 5000,
      plannedActionMinLeadMs: 30_000,
    });
    // Master starts at -3 dB. First call: -3 → +1 = delta +4. That alone
    // exceeds the +3 cap → should be rejected.
    const verdict = sp.evaluateCycle(
      [call("set_master_gain_db", { gain_db: 1 })],
      0,
    );
    expect(verdict.accepted).toHaveLength(0);
    expect(verdict.rejected).toHaveLength(1);
    expect(verdict.rejected[0].reason).toMatch(/per-cycle cap/);
  });

  test("truncates beyond maxToolCallsPerCycle", () => {
    const sp = new SafetyPolicy();
    const tooMany = Array.from({ length: 12 }, (_, i) => call("set_energy", { targetEnergy: i / 12 }));
    const verdict = sp.evaluateCycle(tooMany, 0);
    expect(verdict.accepted).toHaveLength(DEFAULT_SAFETY_CONFIG.maxToolCallsPerCycle);
    expect(verdict.truncated.length).toBe(12 - DEFAULT_SAFETY_CONFIG.maxToolCallsPerCycle);
  });

  test("glicol cooldown blocks rapid re-replace", () => {
    const sp = new SafetyPolicy();
    let v = sp.evaluateCycle([call("create_pattern", { prompt: "x" })], 1000);
    expect(v.accepted).toHaveLength(1);
    v = sp.evaluateCycle([call("create_pattern", { prompt: "y" })], 2000);
    expect(v.accepted).toHaveLength(0);
    expect(v.rejected[0].reason).toMatch(/cooldown/);
    // After cooldown elapses, it is accepted again.
    v = sp.evaluateCycle([call("create_pattern", { prompt: "z" })], 1000 + 5_000 + 1);
    expect(v.accepted).toHaveLength(1);
  });

  test("planned schedule_notes with too-short lead-time is flagged", () => {
    const sp = new SafetyPolicy();
    const c = call("schedule_notes", {
      origin: "PlannedLlm",
      trigger: { kind: "relative", bars: 2 }, // 2 bars @ 120 BPM = 4s
    });
    const v = sp.evaluateCycle([c], 0);
    expect(v.rejected.length).toBe(1);
    expect(v.rejected[0].reason).toMatch(/lead-time/);
  });
});
