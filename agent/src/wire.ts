// End-to-end wire: IPC connection → EngineClient → tool registry →
// PatternStore → SafetyPolicy → DecisionLoop. Lets `index.ts` boot
// the whole stack with one call.

import { DecisionLoop, type LlmCaller, type ToolDispatcher } from "./agent/decision";
import { buildLlmContext, type LlmContext } from "./agent/context";
import { musicTools } from "./agent/extension";
import { PatternStore } from "./pattern/store";
import { ProfileStore } from "./profile/store";
import {
  createEngineClient,
  IpcConnection,
  type EngineClient,
  type IpcConnectOptions,
} from "./ipc";
import type { ToolCall } from "./agent/safety";

export interface WireOptions {
  socketPath?: string;
  requestTimeoutMs?: number;
  /// In-memory by default. Pass a file path for persistence.
  patternStorePath?: string;
  profileStorePath?: string;
  intervalMs?: number;
}

export interface WiredAgent {
  engineClient: EngineClient;
  patternStore: PatternStore;
  profileStore: ProfileStore;
  decisionLoop: DecisionLoop;
  /// All tools registered with the agent. The caller can hand these
  /// to a Pi-SDK session via `customTools: tools` and the
  /// LlmCaller it constructs.
  tools: ReturnType<typeof musicTools>;
  /// Tear everything down: stop decision loop, close stores + socket.
  shutdown: () => Promise<void>;
}

export async function wireAgent(
  caller: LlmCaller,
  options: WireOptions = {},
): Promise<WiredAgent> {
  let connection: IpcConnection | undefined;
  if (options.socketPath) {
    const opts: IpcConnectOptions = {
      socketPath: options.socketPath,
      requestTimeoutMs: options.requestTimeoutMs,
    };
    connection = await IpcConnection.connect(opts);
  }
  const engineClient = createEngineClient(connection);

  // Best-effort handshake. If the engine isn't there, fall back to stub.
  try {
    await engineClient.hello("oh-my-music-agent", "0.1.0");
  } catch {
    /* leave the engineClient as stub; decision loop continues */
  }

  const patternStore = new PatternStore(options.patternStorePath ?? ":memory:");
  const profileStore = new ProfileStore(options.profileStorePath ?? ":memory:");
  const tools = musicTools(engineClient, patternStore);

  // Dispatcher used by the DecisionLoop: walk the tool registry to
  // find the matching tool, hand it the args.  Tools have already
  // validated args via TypeBox so the dispatch is mechanical.
  const toolsByName = new Map(tools.map((t) => [t.name, t] as const));
  const dispatcher: ToolDispatcher = {
    async dispatch(call) {
      const tool = toolsByName.get(call.name);
      if (!tool) throw new Error(`unknown tool: ${call.name}`);
      // Pi-SDK tools take (toolCallId, params, signal, onUpdate, ctx).
      // We pass minimal stand-ins — the loop is the only caller here.
      const result = await (tool as any).execute(
        `auto-${Date.now()}`,
        call.args,
        undefined,
        undefined,
        undefined,
      );
      return result?.content?.[0]?.text ?? call.name;
    },
  };

  // Context builder defaults to defaultLlmContext (no live analysis yet).
  // Callers that have actual sensors can wrap this with their own
  // buildContext closure.
  const buildContext = (): LlmContext =>
    buildLlmContext({
      mode: "tui",
      userGoal: null,
      state: {
        energy: 0.5,
        masterPeakDb: -60,
        limiterGainReductionDb: 0,
        activeSources: [],
        glicolCodeLoaded: false,
        glicolActiveChains: [],
        playerPlaying: false,
      },
      environment: {
        label: "unknown",
        confidence: 0,
        speechProbability: 0,
        noiseFloorDb: -60,
      },
      features: { last2s: {}, last10s: {}, trend: "stable" },
      safety: {
        clipping: false,
        queuePressure: 0,
        captureStatus: { systemAudio: "idle", mic: "idle" },
      },
    });

  const decisionLoop = new DecisionLoop(caller, dispatcher, buildContext, {
    intervalMs: options.intervalMs,
  });

  return {
    engineClient,
    patternStore,
    profileStore,
    decisionLoop,
    tools,
    async shutdown() {
      decisionLoop.stop();
      patternStore.close();
      profileStore.close();
      engineClient.close();
    },
  };
}

/// Convenience: a no-op LlmCaller that emits no tool calls. Useful
/// for `bun run src/index.ts` smoke tests where no real LLM is
/// configured yet.
export const noopLlmCaller: LlmCaller = {
  async prompt(): Promise<ToolCall[]> {
    return [];
  },
};
