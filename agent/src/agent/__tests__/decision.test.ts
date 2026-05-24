import { describe, expect, test } from "bun:test";

import { DecisionLoop, type LlmCaller, type ToolDispatcher } from "../decision";
import { defaultLlmContext, type LlmContext } from "../context";
import type { ToolCall } from "../safety";

function mockCaller(returnValues: ToolCall[][]): LlmCaller {
  let i = 0;
  return {
    async prompt(_input) {
      const v = returnValues[Math.min(i, returnValues.length - 1)];
      i += 1;
      return v;
    },
  };
}

function mockDispatcher(): { dispatcher: ToolDispatcher; calls: ToolCall[] } {
  const calls: ToolCall[] = [];
  return {
    calls,
    dispatcher: {
      async dispatch(call) {
        calls.push(call);
        return call.name;
      },
    },
  };
}

describe("DecisionLoop", () => {
  test("applies LLM tool calls to the dispatcher", async () => {
    const caller = mockCaller([
      [
        { name: "set_energy", args: { targetEnergy: 0.6 } },
        { name: "schedule_notes", args: { origin: "Manual" } },
      ],
    ]);
    const { dispatcher, calls } = mockDispatcher();
    const ctxFn: () => LlmContext = () => defaultLlmContext("tui");
    const loop = new DecisionLoop(caller, dispatcher, ctxFn);
    const report = await loop.tick();
    expect(report).not.toBeNull();
    expect(report!.accepted).toHaveLength(2);
    expect(calls.map((c) => c.name)).toEqual(["set_energy", "schedule_notes"]);
  });

  test("safety policy rejects offending calls but applies the rest", async () => {
    const caller = mockCaller([
      [
        { name: "set_master_gain_db", args: { gain_db: 12 } }, // rejected (above ceiling)
        { name: "set_energy", args: { targetEnergy: 0.5 } }, // accepted
      ],
    ]);
    const { dispatcher, calls } = mockDispatcher();
    const loop = new DecisionLoop(caller, dispatcher, () => defaultLlmContext());
    const report = await loop.tick();
    expect(report!.accepted.map((c) => c.name)).toEqual(["set_energy"]);
    expect(report!.rejected.map((r) => r.call.name)).toEqual(["set_master_gain_db"]);
    expect(calls).toHaveLength(1);
  });

  test("LLM errors are recorded as a no-op decision", async () => {
    const caller: LlmCaller = {
      async prompt() {
        throw new Error("LLM unavailable");
      },
    };
    const { dispatcher, calls } = mockDispatcher();
    const loop = new DecisionLoop(caller, dispatcher, () => defaultLlmContext());
    const report = await loop.tick();
    expect(report!.accepted).toHaveLength(0);
    expect(calls).toHaveLength(0);
    expect(loop.recentDecisionsView()[0].summary).toMatch(/LLM call failed/);
  });

  test("dispatch failures surface in dispatchErrors but do not stop the cycle", async () => {
    const caller = mockCaller([[
      { name: "set_energy", args: { targetEnergy: 0.5 } },
      { name: "schedule_notes", args: { origin: "Manual" } },
    ]]);
    const dispatcher: ToolDispatcher = {
      async dispatch(call) {
        if (call.name === "schedule_notes") throw new Error("queue full");
        return call.name;
      },
    };
    const loop = new DecisionLoop(caller, dispatcher, () => defaultLlmContext());
    const report = await loop.tick();
    expect(report!.accepted).toHaveLength(2);
    expect(report!.dispatchErrors).toHaveLength(1);
    expect(report!.dispatchErrors[0].call.name).toBe("schedule_notes");
  });

  test("enqueueUserInput delivers the latest command to the LLM", async () => {
    const received: (string | null)[] = [];
    const caller: LlmCaller = {
      async prompt(input) {
        received.push(input.userInput);
        return [];
      },
    };
    const { dispatcher } = mockDispatcher();
    const loop = new DecisionLoop(caller, dispatcher, () => defaultLlmContext());
    loop.enqueueUserInput("make it warmer");
    await loop.tick();
    expect(received[0]).toBe("make it warmer");
    // Second cycle without a new input → null
    await loop.tick();
    expect(received[1]).toBe(null);
  });

  test("recordDecision keeps at most historyLimit entries", async () => {
    const caller = mockCaller([[]]);
    const { dispatcher } = mockDispatcher();
    const loop = new DecisionLoop(caller, dispatcher, () => defaultLlmContext(), {
      historyLimit: 3,
    });
    for (let i = 0; i < 5; i++) {
      await loop.tick();
    }
    expect(loop.recentDecisionsView()).toHaveLength(3);
  });
});
