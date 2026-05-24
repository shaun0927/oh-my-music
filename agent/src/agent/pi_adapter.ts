// Adapter that wraps a Pi-SDK `AgentSession` as an `LlmCaller` so the
// DecisionLoop can drive it. Tool execution is delegated to the
// Pi-SDK (which calls the registered `defineTool` execute() methods
// directly). The returned ToolCall list is therefore always empty —
// the DecisionLoop's SafetyPolicy is bypassed because Pi-SDK does
// not expose a pre-execute hook for arbitrary tools.
//
// If you want SafetyPolicy enforcement on the way out, wrap your
// individual tools' execute() with a SafetyPolicy.checkCall() call
// — that's where it lives in the Pi-SDK model.

import type { LlmCaller } from "./decision";
import type { LlmContext } from "./context";
import type { ToolCall } from "./safety";

/// The minimal subset of `AgentSession` we depend on. Tests stub
/// this without pulling in the real Pi-SDK.
export interface PiSessionLike {
  /// Send a prompt to the agent; Pi runs any registered tools as a
  /// side effect. Returns when the agent finishes its turn.
  prompt(text: string, options?: Record<string, unknown>): Promise<void>;
}

/// Render an LlmContext as a JSON system-style preamble so the
/// model sees the same state shape on every cycle. Single line per
/// section so the model can scan it quickly.
export function renderContextPreamble(context: LlmContext, userInput: string | null): string {
  const lines = [
    `# Current state`,
    `mode: ${context.mode}`,
    `goal: ${context.userGoal ?? "(none)"}`,
    `energy: ${context.currentState.energy.toFixed(2)} | bpm: ${context.currentState.bpm ?? "?"} | key: ${context.currentState.key ?? "?"}`,
    `master_peak_db: ${context.currentState.masterPeakDb.toFixed(1)} | limiter_gr_db: ${context.currentState.limiterGainReductionDb.toFixed(1)}`,
    `active_sources: ${context.currentState.activeSources.join(", ") || "(none)"}`,
    `glicol: loaded=${context.currentState.glicolCodeLoaded} chains=${context.currentState.glicolActiveChains.join(", ") || "(none)"}`,
    `environment: ${context.environment.label} (conf ${context.environment.confidence.toFixed(2)})`,
    `trend: ${context.recentFeatures.trend}`,
    `safety: clipping=${context.safety.clipping} queue_pressure=${context.safety.queuePressure.toFixed(2)}`,
    `recent_decisions: ${context.recentDecisions.map((d) => d.summary).join(" | ") || "(none)"}`,
  ];
  if (userInput) {
    lines.push("", `# User input`, userInput);
  } else {
    lines.push("", `# Cycle action`, "Decide whether any tool calls are needed this cycle. Make none if state is acceptable.");
  }
  return lines.join("\n");
}

export interface PiSdkLlmCallerOptions {
  /// Pi-SDK prompt() options passed verbatim (e.g. streamingBehavior).
  promptOptions?: Record<string, unknown>;
  /// Override the prompt-text builder. Useful for callers that want
  /// a different prompt template than the default JSON-line preamble.
  buildPrompt?: (context: LlmContext, userInput: string | null) => string;
}

/// Adapter constructor. Returns an LlmCaller that the DecisionLoop
/// can drive. Pi-SDK handles tool execution during prompt(); the
/// caller returns [] so the loop's SafetyPolicy/dispatch loop is a
/// no-op for Pi-SDK-driven cycles.
export function createPiSdkLlmCaller(
  session: PiSessionLike,
  options: PiSdkLlmCallerOptions = {},
): LlmCaller {
  return {
    async prompt({ context, userInput }) {
      const text = (options.buildPrompt ?? renderContextPreamble)(context, userInput);
      await session.prompt(text, options.promptOptions);
      // Pi-SDK already executed any tool calls during the await.
      // The DecisionLoop's verdict ledger therefore receives an empty
      // accepted list. Tool execution side effects (engineClient
      // calls, PatternStore writes) have already happened.
      const empty: ToolCall[] = [];
      return empty;
    },
  };
}
