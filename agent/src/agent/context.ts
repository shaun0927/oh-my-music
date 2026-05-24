// LLM context builder per `docs/ARCHITECTURE.md` §9.4. Collects the
// runtime snapshot, environment label, user goal/profile and recent
// decisions into one JSON blob the LLM consumes per cycle.

export type SessionMode = "tui" | "discord";

export type EnvironmentLabel =
  | "quiet"
  | "cafe"
  | "outdoor"
  | "speech"
  | "music"
  | "noise"
  | "unknown";

export interface FeatureSummary {
  centroidHz?: number;
  rmsDb?: number;
  onsetRatePerSec?: number;
  brightness?: "dark" | "neutral" | "bright";
  energy?: number;
}

export interface CurrentState {
  energy: number;
  bpm?: number;
  key?: string;
  masterPeakDb: number;
  limiterGainReductionDb: number;
  activeSources: string[];
  glicolCodeLoaded: boolean;
  glicolActiveChains: string[];
  playerPlaying: boolean;
  playerTrackName?: string;
  playerPositionSec?: number;
}

export interface RecentDecision {
  at: string;
  summary: string;
  tools: string[];
}

export interface LlmContext {
  mode: SessionMode;
  userGoal: string | null;
  currentState: CurrentState;
  environment: {
    label: EnvironmentLabel;
    confidence: number;
    speechProbability: number;
    noiseFloorDb: number;
  };
  recentFeatures: {
    last2s: FeatureSummary;
    last10s: FeatureSummary;
    trend: "rising" | "falling" | "stable";
  };
  userPreferences: Record<string, unknown>;
  recentUserCommands: string[];
  recentDecisions: RecentDecision[];
  safety: {
    clipping: boolean;
    queuePressure: number;
    captureStatus: { systemAudio: string; mic: string };
  };
}

export interface BuildContextInput {
  mode: SessionMode;
  userGoal: string | null;
  state: CurrentState;
  environment: LlmContext["environment"];
  features: LlmContext["recentFeatures"];
  userPreferences?: Record<string, unknown>;
  recentUserCommands?: string[];
  recentDecisions?: RecentDecision[];
  safety: LlmContext["safety"];
}

export function buildLlmContext(input: BuildContextInput): LlmContext {
  return {
    mode: input.mode,
    userGoal: input.userGoal,
    currentState: input.state,
    environment: input.environment,
    recentFeatures: input.features,
    userPreferences: input.userPreferences ?? {},
    recentUserCommands: (input.recentUserCommands ?? []).slice(-5),
    recentDecisions: (input.recentDecisions ?? []).slice(-5),
    safety: input.safety,
  };
}

export function defaultLlmContext(mode: SessionMode = "tui"): LlmContext {
  return buildLlmContext({
    mode,
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
    features: {
      last2s: {},
      last10s: {},
      trend: "stable",
    },
    safety: {
      clipping: false,
      queuePressure: 0,
      captureStatus: { systemAudio: "idle", mic: "idle" },
    },
  });
}

// Back-compat for older code that referenced the original simpler
// `MusicAgentContext` shape.
export type MusicAgentContext = {
  mode: SessionMode;
  energy: number;
  environment: EnvironmentLabel;
};

export function defaultContext(): MusicAgentContext {
  return { mode: "tui", energy: 0.5, environment: "unknown" };
}
