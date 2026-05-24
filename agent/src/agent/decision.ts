// LLM decision loop per `docs/ARCHITECTURE.md` §9.2 + §9.3.
//
// The loop is pluggable: the actual Pi-SDK session call is exposed
// as `LlmCaller`, an interface tests stub out. Real LLM integration
// (via `createMusicAgent`) wraps the Pi session into an LlmCaller.

import { createAgentSession, SessionManager } from "@mariozechner/pi-coding-agent";

import { createEngineClient } from "../engine-client";
import { musicTools } from "./extension";
import type { LlmContext, RecentDecision } from "./context";
import { SafetyPolicy, type ToolCall } from "./safety";

export interface LlmCaller {
  prompt(input: { context: LlmContext; userInput: string | null }): Promise<ToolCall[]>;
}

export interface ToolDispatcher {
  dispatch(call: ToolCall): Promise<string>;
}

export interface DecisionLoopOptions {
  intervalMs?: number;
  maxConcurrentPrompts?: number;
  historyLimit?: number;
}

const DEFAULT_OPTIONS: Required<DecisionLoopOptions> = {
  intervalMs: 2000,
  maxConcurrentPrompts: 1,
  historyLimit: 5,
};

export interface CycleReport {
  startedAt: number;
  finishedAt: number;
  accepted: ToolCall[];
  rejected: { call: ToolCall; reason: string }[];
  truncated: ToolCall[];
  dispatchErrors: { call: ToolCall; message: string }[];
}

export class DecisionLoop {
  private timer: ReturnType<typeof setInterval> | null = null;
  private inFlight = 0;
  private safety: SafetyPolicy;
  private recentDecisions: RecentDecision[] = [];
  private nextUserInput: string | null = null;
  private readonly opts: Required<DecisionLoopOptions>;

  constructor(
    private readonly caller: LlmCaller,
    private readonly dispatcher: ToolDispatcher,
    private readonly buildContext: () => LlmContext,
    opts: DecisionLoopOptions = {},
    safety?: SafetyPolicy,
  ) {
    this.opts = { ...DEFAULT_OPTIONS, ...opts };
    this.safety = safety ?? new SafetyPolicy();
  }

  start(): void {
    if (this.timer) return;
    this.timer = setInterval(() => {
      void this.tick();
    }, this.opts.intervalMs);
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  enqueueUserInput(input: string): void {
    this.nextUserInput = input;
  }

  async tick(): Promise<CycleReport | null> {
    if (this.inFlight >= this.opts.maxConcurrentPrompts) {
      return null;
    }
    const startedAt = Date.now();
    this.inFlight += 1;
    const context = this.buildContext();
    const userInput = this.nextUserInput;
    this.nextUserInput = null;
    let toolCalls: ToolCall[] = [];
    try {
      toolCalls = await this.caller.prompt({ context, userInput });
    } catch (err) {
      this.inFlight -= 1;
      this.recordDecision({
        at: new Date(startedAt).toISOString(),
        summary: `LLM call failed: ${(err as Error).message}`,
        tools: [],
      });
      return {
        startedAt,
        finishedAt: Date.now(),
        accepted: [],
        rejected: [],
        truncated: [],
        dispatchErrors: [],
      };
    }
    const verdict = this.safety.evaluateCycle(toolCalls, startedAt);
    const dispatchErrors: CycleReport["dispatchErrors"] = [];
    const labels: string[] = [];
    for (const call of verdict.accepted) {
      try {
        const label = await this.dispatcher.dispatch(call);
        labels.push(label || call.name);
      } catch (err) {
        dispatchErrors.push({ call, message: (err as Error).message });
      }
    }
    this.recordDecision({
      at: new Date(startedAt).toISOString(),
      summary: labels.length
        ? `applied ${labels.length} tool(s): ${labels.join(", ")}`
        : "no tools applied",
      tools: verdict.accepted.map((c) => c.name),
    });
    this.inFlight -= 1;
    return {
      startedAt,
      finishedAt: Date.now(),
      accepted: verdict.accepted,
      rejected: verdict.rejected,
      truncated: verdict.truncated,
      dispatchErrors,
    };
  }

  recentDecisionsView(): RecentDecision[] {
    return this.recentDecisions.slice();
  }

  private recordDecision(d: RecentDecision): void {
    this.recentDecisions.push(d);
    if (this.recentDecisions.length > this.opts.historyLimit) {
      this.recentDecisions.shift();
    }
  }
}

// -- Back-compat: original createMusicAgent entry point -------------------

export async function createMusicAgent() {
  const engineClient = createEngineClient();
  const { session } = await createAgentSession({
    customTools: musicTools(engineClient),
    sessionManager: SessionManager.inMemory(),
  });

  return {
    session,
    sessionId: session.sessionId,
  };
}
