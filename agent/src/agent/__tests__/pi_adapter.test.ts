import { describe, expect, test } from "bun:test";

import { createPiSdkLlmCaller, renderContextPreamble, type PiSessionLike } from "../pi_adapter";
import { defaultLlmContext } from "../context";

describe("renderContextPreamble", () => {
  test("includes user input when provided", () => {
    const text = renderContextPreamble(defaultLlmContext("tui"), "make it warmer");
    expect(text).toContain("# User input");
    expect(text).toContain("make it warmer");
  });

  test("falls back to cycle-action prompt when user input is null", () => {
    const text = renderContextPreamble(defaultLlmContext("tui"), null);
    expect(text).toContain("# Cycle action");
    expect(text).not.toContain("# User input");
  });

  test("includes state, environment, safety, recent_decisions lines", () => {
    const text = renderContextPreamble(defaultLlmContext("tui"), null);
    expect(text).toContain("active_sources:");
    expect(text).toContain("environment:");
    expect(text).toContain("safety:");
    expect(text).toContain("recent_decisions:");
  });
});

describe("createPiSdkLlmCaller", () => {
  test("forwards prompt text to the session and returns empty tool list", async () => {
    let received: string | undefined;
    let receivedOptions: Record<string, unknown> | undefined;
    const session: PiSessionLike = {
      async prompt(text, options) {
        received = text;
        receivedOptions = options;
      },
    };
    const caller = createPiSdkLlmCaller(session);
    const ctx = defaultLlmContext("tui");
    const calls = await caller.prompt({ context: ctx, userInput: "hi" });
    expect(calls).toEqual([]);
    expect(received).toContain("hi");
    expect(receivedOptions).toBeUndefined();
  });

  test("buildPrompt override is respected", async () => {
    let received: string | undefined;
    const session: PiSessionLike = {
      async prompt(text) {
        received = text;
      },
    };
    const caller = createPiSdkLlmCaller(session, {
      buildPrompt: () => "custom static prompt",
    });
    await caller.prompt({ context: defaultLlmContext(), userInput: null });
    expect(received).toBe("custom static prompt");
  });

  test("promptOptions flow through to session.prompt()", async () => {
    let receivedOptions: Record<string, unknown> | undefined;
    const session: PiSessionLike = {
      async prompt(_text, options) {
        receivedOptions = options;
      },
    };
    const caller = createPiSdkLlmCaller(session, {
      promptOptions: { streamingBehavior: "followUp" },
    });
    await caller.prompt({ context: defaultLlmContext(), userInput: null });
    expect(receivedOptions).toEqual({ streamingBehavior: "followUp" });
  });

  test("session errors propagate up to the loop", async () => {
    const session: PiSessionLike = {
      async prompt() {
        throw new Error("model offline");
      },
    };
    const caller = createPiSdkLlmCaller(session);
    await expect(
      caller.prompt({ context: defaultLlmContext(), userInput: null }),
    ).rejects.toThrow(/model offline/);
  });
});
