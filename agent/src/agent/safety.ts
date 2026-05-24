// Safety policy applied to LLM-issued tool calls.  Implements
// `docs/ARCHITECTURE.md` §9.3 + §13.x: rate limits, master-gain cap,
// per-cycle gain increase cap, tool-call cap, and a Glicol-code
// replacement cool-down.

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export interface SafetyConfig {
  /// Maximum cumulative gain increase the LLM may apply in a single
  /// decision cycle, in dB. Default +3 dB.
  maxGainIncreasePerCycleDb: number;
  /// Hard ceiling on master gain. The LLM cannot raise master above
  /// this value via any tool call. Default 0 dB.
  masterGainCeilingDb: number;
  /// Maximum number of tool calls the LLM may issue per cycle.
  maxToolCallsPerCycle: number;
  /// Minimum gap between Glicol-code replacements. Default 5 s.
  glicolCooldownMs: number;
  /// Minimum lead-time for planned actions in ms (matches
  /// `crates/omm-protocol/src/scheduler.rs` PLANNED_ACTION_MIN_LEAD_MS).
  plannedActionMinLeadMs: number;
}

export const DEFAULT_SAFETY_CONFIG: SafetyConfig = {
  maxGainIncreasePerCycleDb: 3,
  masterGainCeilingDb: 0,
  maxToolCallsPerCycle: 8,
  glicolCooldownMs: 5000,
  plannedActionMinLeadMs: 30_000,
};

export interface ToolCall {
  /// Tool name as registered with the Pi SDK (e.g. `set_energy`,
  /// `schedule_notes`).
  name: string;
  /// Pre-validated argument bag (already clamped by the tool's own
  /// `clamp(...)` calls). SafetyPolicy makes cross-call decisions.
  args: Record<string, unknown>;
}

export interface SafetyVerdict {
  accepted: ToolCall[];
  rejected: { call: ToolCall; reason: string }[];
  truncated: ToolCall[];
}

export class SafetyPolicy {
  private cumulativeGainIncreaseDb = 0;
  private lastGlicolReplaceAt: number | null = null;
  private masterGainEstimateDb = -3; // ARCH 6.4 default master gain
  private readonly cfg: SafetyConfig;

  constructor(cfg: SafetyConfig = DEFAULT_SAFETY_CONFIG) {
    this.cfg = cfg;
  }

  beginCycle(): void {
    this.cumulativeGainIncreaseDb = 0;
  }

  evaluateCycle(calls: ToolCall[], nowMs: number): SafetyVerdict {
    this.beginCycle();
    const accepted: ToolCall[] = [];
    const rejected: SafetyVerdict["rejected"] = [];
    const truncated: ToolCall[] = [];

    for (const call of calls) {
      if (accepted.length >= this.cfg.maxToolCallsPerCycle) {
        truncated.push(call);
        continue;
      }
      const reason = this.checkCall(call, nowMs);
      if (reason) {
        rejected.push({ call, reason });
      } else {
        this.applyCall(call, nowMs);
        accepted.push(call);
      }
    }

    return { accepted, rejected, truncated };
  }

  private checkCall(call: ToolCall, nowMs: number): string | null {
    if (call.name === "set_master_gain_db") {
      const target = num(call.args.gain_db ?? call.args.targetDb);
      if (target === null) return "set_master_gain_db missing numeric gain_db";
      if (target > this.cfg.masterGainCeilingDb) {
        return `master gain ${target} dB exceeds ceiling ${this.cfg.masterGainCeilingDb} dB`;
      }
      const delta = target - this.masterGainEstimateDb;
      if (delta > 0 && this.cumulativeGainIncreaseDb + delta > this.cfg.maxGainIncreasePerCycleDb) {
        return `gain increase ${delta.toFixed(2)} dB would exceed per-cycle cap ${this.cfg.maxGainIncreasePerCycleDb} dB`;
      }
    }
    if (call.name === "create_pattern" || call.name === "modify_pattern") {
      if (
        this.lastGlicolReplaceAt !== null &&
        nowMs - this.lastGlicolReplaceAt < this.cfg.glicolCooldownMs
      ) {
        const remain = this.cfg.glicolCooldownMs - (nowMs - this.lastGlicolReplaceAt);
        return `glicol cooldown active (${remain} ms remaining)`;
      }
    }
    if (call.name === "schedule_notes") {
      const origin = (call.args.origin as string | undefined) ?? "Manual";
      if (origin === "PlannedLlm" || origin === "PlannedPi") {
        const trigger = call.args.trigger as { kind?: string; bars?: number } | undefined;
        if (
          trigger &&
          trigger.kind === "relative" &&
          (trigger.bars ?? 0) * (60_000 / 120) < this.cfg.plannedActionMinLeadMs
        ) {
          return `planned action under ${this.cfg.plannedActionMinLeadMs} ms lead-time`;
        }
      }
    }
    return null;
  }

  private applyCall(call: ToolCall, nowMs: number): void {
    if (call.name === "set_master_gain_db") {
      const target = num(call.args.gain_db ?? call.args.targetDb);
      if (target !== null) {
        const delta = target - this.masterGainEstimateDb;
        if (delta > 0) this.cumulativeGainIncreaseDb += delta;
        this.masterGainEstimateDb = target;
      }
    }
    if (call.name === "create_pattern" || call.name === "modify_pattern") {
      this.lastGlicolReplaceAt = nowMs;
    }
  }
}

function num(v: unknown): number | null {
  if (typeof v === "number" && Number.isFinite(v)) return v;
  return null;
}
