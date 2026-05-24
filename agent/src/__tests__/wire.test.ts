import { describe, expect, test } from "bun:test";

import { noopLlmCaller, wireAgent } from "../wire";
import type { ToolCall } from "../agent/safety";
import type { LlmCaller } from "../agent/decision";

describe("wireAgent", () => {
  test("boots without a socket and runs one decision tick", async () => {
    const wired = await wireAgent(noopLlmCaller);
    expect(wired.tools.length).toBeGreaterThan(0);
    const report = await wired.decisionLoop.tick();
    expect(report).not.toBeNull();
    expect(report!.accepted).toHaveLength(0);
    await wired.shutdown();
  });

  test("dispatches accepted tool calls to the tool registry", async () => {
    let savedId: string | undefined;
    const caller: LlmCaller = {
      async prompt() {
        const call: ToolCall = {
          name: "save_pattern",
          args: {
            id: "test-1",
            name: "Test",
            role: "motif",
            notes: [
              { pitch: 60, velocity: 100, start_ticks: 0, length_ticks: 480 },
            ],
            lengthBars: 1,
            tags: [],
          },
        };
        return [call];
      },
    };
    const wired = await wireAgent(caller);
    const report = await wired.decisionLoop.tick();
    expect(report!.accepted).toHaveLength(1);
    const stored = wired.patternStore.recall("test-1");
    expect(stored).not.toBeNull();
    expect(stored!.name).toBe("Test");
    savedId = stored!.id;
    expect(savedId).toBe("test-1");
    await wired.shutdown();
  });

  test("PatternStore + ProfileStore + EngineClient all close on shutdown", async () => {
    const wired = await wireAgent(noopLlmCaller);
    // Nothing throws; this is the contract test.
    await wired.shutdown();
  });
});
