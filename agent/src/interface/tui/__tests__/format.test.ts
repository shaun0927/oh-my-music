import { describe, expect, test } from "bun:test";

import {
  decisionsToLines,
  formatDb,
  formatSourceRow,
  renderMeterBar,
  statusLine,
} from "../format";
import { defaultLlmContext } from "../../../agent/context";

describe("renderMeterBar", () => {
  test("silence renders an empty bar", () => {
    expect(renderMeterBar(-60, 10)).toBe("░".repeat(10));
  });

  test("0 dB renders a full bar", () => {
    expect(renderMeterBar(0, 10)).toBe("█".repeat(10));
  });

  test("midpoint -30 dB ≈ half full", () => {
    const s = renderMeterBar(-30, 10);
    const filled = s.split("█").length - 1;
    expect(filled).toBe(5);
  });

  test("values below floor clamp to empty", () => {
    expect(renderMeterBar(-200, 8)).toBe("░".repeat(8));
  });

  test("values above 0 dB clamp to full", () => {
    expect(renderMeterBar(20, 8)).toBe("█".repeat(8));
  });
});

describe("formatDb", () => {
  test("right-pads to width 6", () => {
    expect(formatDb(-12.3).length).toBe(6);
    expect(formatDb(0).length).toBe(6);
  });

  test("renders -12.3 as ' -12.3'", () => {
    expect(formatDb(-12.3)).toBe(" -12.3");
  });
});

describe("statusLine", () => {
  test("includes env label and energy from defaults", () => {
    const s = statusLine(defaultLlmContext("tui"));
    expect(s).toContain("Env unknown");
    expect(s).toContain("Energy 0.50");
  });

  test("renders BPM and Key when set", () => {
    const ctx = defaultLlmContext("discord");
    ctx.currentState.bpm = 120;
    ctx.currentState.key = "Am";
    expect(statusLine(ctx)).toContain("BPM 120");
    expect(statusLine(ctx)).toContain("Key Am");
  });
});

describe("decisionsToLines", () => {
  test("returns latest-first within limit", () => {
    const decisions = Array.from({ length: 8 }, (_, i) => ({
      at: new Date(1_700_000_000_000 + i * 1000).toISOString(),
      summary: `step ${i}`,
      tools: [],
    }));
    const lines = decisionsToLines(decisions, 3);
    expect(lines).toHaveLength(3);
    expect(lines[0]).toContain("step 7");
    expect(lines[2]).toContain("step 5");
  });

  test("returns empty for no decisions", () => {
    expect(decisionsToLines([], 3)).toEqual([]);
  });
});

describe("formatSourceRow", () => {
  test("includes label, bar, db, flags, ornament", () => {
    const row = formatSourceRow({
      label: "Player",
      db: -8.2,
      flags: "solo",
      ornament: "♫ lo-fi.mp3",
    });
    expect(row).toContain("Player");
    expect(row).toContain("-8.2 dB");
    expect(row).toContain("[solo]");
    expect(row).toContain("♫ lo-fi.mp3");
  });

  test("omits flags / ornament when undefined", () => {
    const row = formatSourceRow({ label: "Mic", db: -32.1 });
    expect(row).toContain("Mic");
    expect(row).toContain("-32.1 dB");
    expect(row).not.toContain("[");
    expect(row).not.toContain("♫");
  });
});
