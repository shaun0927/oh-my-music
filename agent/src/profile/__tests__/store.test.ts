import { describe, expect, test } from "bun:test";

import { ProfileStore, type Profile, type DecisionLogEntry } from "../store";

function sampleProfile(id: string): Omit<Profile, "createdAt" | "updatedAt"> {
  return {
    id,
    scope: "local-user",
    preferredEnergy: 0.6,
    volumeLimitDb: -3,
    favoriteStyles: ["lo-fi", "ambient"],
  };
}

describe("ProfileStore", () => {
  test("upsert + get round-trips a profile", () => {
    const s = new ProfileStore();
    s.upsertProfile(sampleProfile("u1"));
    const got = s.getProfile("u1");
    expect(got).not.toBeNull();
    expect(got!.preferredEnergy).toBe(0.6);
    expect(got!.favoriteStyles).toEqual(["lo-fi", "ambient"]);
  });

  test("upsert preserves createdAt across updates", () => {
    const s = new ProfileStore();
    const first = s.upsertProfile(sampleProfile("u1"));
    const second = s.upsertProfile({ ...sampleProfile("u1"), preferredEnergy: 0.9 });
    expect(second.createdAt).toBe(first.createdAt);
    expect(second.preferredEnergy).toBe(0.9);
  });

  test("listProfiles filters by scope", () => {
    const s = new ProfileStore();
    s.upsertProfile(sampleProfile("u1"));
    s.upsertProfile({ ...sampleProfile("u2"), scope: "discord-user", discordUserId: "abc" });
    expect(s.listProfiles("local-user")).toHaveLength(1);
    expect(s.listProfiles("discord-user")).toHaveLength(1);
  });

  test("removeProfile returns true on success and false on missing", () => {
    const s = new ProfileStore();
    s.upsertProfile(sampleProfile("u1"));
    expect(s.removeProfile("u1")).toBe(true);
    expect(s.removeProfile("u1")).toBe(false);
  });

  test("appendDecision + recentDecisions returns chronological order", () => {
    const s = new ProfileStore();
    const ts = (offset: number) => new Date(1_700_000_000_000 + offset).toISOString();
    const make = (i: number): DecisionLogEntry => ({
      sessionId: "sess-a",
      createdAt: ts(i * 1000),
      contextJson: JSON.stringify({ i }),
      toolCallsJson: JSON.stringify({ tools: [] }),
      rationale: `step ${i}`,
    });
    for (let i = 0; i < 5; i++) s.appendDecision(make(i));
    const recent = s.recentDecisions("sess-a", 3);
    expect(recent).toHaveLength(3);
    // Oldest first within the returned window (id DESC then reverse).
    expect(recent[0].rationale).toBe("step 2");
    expect(recent[2].rationale).toBe("step 4");
  });

  test("decisionCount tracks per-session and global", () => {
    const s = new ProfileStore();
    const ts = (i: number) => new Date(1_700_000_000_000 + i * 1000).toISOString();
    for (let i = 0; i < 3; i++) {
      s.appendDecision({
        sessionId: "a",
        createdAt: ts(i),
        contextJson: "{}",
        toolCallsJson: "[]",
        rationale: null,
      });
    }
    for (let i = 0; i < 2; i++) {
      s.appendDecision({
        sessionId: "b",
        createdAt: ts(i),
        contextJson: "{}",
        toolCallsJson: "[]",
        rationale: null,
      });
    }
    expect(s.decisionCount("a")).toBe(3);
    expect(s.decisionCount("b")).toBe(2);
    expect(s.decisionCount()).toBe(5);
  });
});
