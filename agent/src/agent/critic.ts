// Critic LLM (Epic #1 Phase 7 / D4).
//
// A second LLM session listens to the offline-rendered audio for the
// most recent block, compares it against the user-provided style brief,
// and produces aesthetic feedback the Composer reads on the next
// section. Self-approval is forbidden — the Critic must be a
// different session from the Composer (different prompt, ideally a
// different model).

import type { ToolCall } from "./safety";

export interface RenderObservation {
  /// What block this observation is about.
  sectionIndex: number;
  /// Deterministic numeric metrics from `verdict::judge`. Critic uses
  /// them as objective ground truth — its job is the *aesthetic*
  /// layer above this.
  peak: number;
  rms: number;
  dynamicRangeDb: number;
  clippedSampleRatio: number;
  silenceRatio: number;
  estimatedOnsetRatePerSec: number;
  /// Deterministic-verdict pass/fail. If false, the Critic still
  /// reports but the orchestrator should not commit the block.
  deterministicPassed: boolean;
}

export interface CritiqueFeedback {
  sectionIndex: number;
  /// "accept" / "revise" / "reject". Composer interprets:
  /// - accept: commit as-is
  /// - revise: invalidate the SectionPlan and try again with notes
  /// - reject: invalidate AND mark a stronger correction needed
  verdict: "accept" | "revise" | "reject";
  /// Short natural-language note the Composer reads on its next pass.
  note: string;
  /// Optional tool calls the Critic suggests applying before re-render
  /// (e.g. "set_energy lower"). Subject to SafetyPolicy like everything
  /// else.
  suggestedFollowUps: ToolCall[];
}

export interface CriticCaller {
  critique(input: {
    brief: string;
    observation: RenderObservation;
  }): Promise<CritiqueFeedback>;
}

/// Orchestration helper: feed an observation through the Critic and
/// keep a sliding history of feedback per section. Composer pulls the
/// latest feedback before re-planning.
export class CriticOrchestrator {
  private feedbackBySection = new Map<number, CritiqueFeedback[]>();

  constructor(private readonly caller: CriticCaller) {}

  async observe(
    brief: string,
    observation: RenderObservation,
  ): Promise<CritiqueFeedback> {
    const feedback = await this.caller.critique({ brief, observation });
    const list = this.feedbackBySection.get(observation.sectionIndex) ?? [];
    list.push(feedback);
    this.feedbackBySection.set(observation.sectionIndex, list);
    return feedback;
  }

  /// All feedback received for `sectionIndex` so far (oldest first).
  feedbackFor(sectionIndex: number): CritiqueFeedback[] {
    return (this.feedbackBySection.get(sectionIndex) ?? []).slice();
  }

  latestFeedback(sectionIndex: number): CritiqueFeedback | null {
    const list = this.feedbackBySection.get(sectionIndex);
    return list && list.length ? list[list.length - 1] : null;
  }

  /// Helper for the Composer: read the last verdict for `sectionIndex`
  /// and turn it into a yes/no decision.
  shouldCommit(sectionIndex: number, deterministicPassed: boolean): boolean {
    if (!deterministicPassed) return false;
    const fb = this.latestFeedback(sectionIndex);
    if (!fb) return true; // no critic feedback yet — assume OK
    return fb.verdict === "accept";
  }
}
