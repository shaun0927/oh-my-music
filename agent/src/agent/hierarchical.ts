// 3-tier hierarchical planner: Director → Composer → Performer.
//
// Director runs once per song (input: user brief → FormPlan).
// Composer runs once per section (FormPlan + sectionIndex → SectionPlan).
// Performer runs per cycle (handled by the existing DecisionLoop in
// `decision.ts`).
//
// All three roles are pluggable callers so tests stub them out; real
// use plugs in Pi-SDK sessions configured with different models
// (Opus for Director/Composer, Sonnet/Haiku for Performer).

import type { FormPlan, SectionOutline, SectionPlan } from "./form";
import type { ToolCall } from "./safety";

export interface DirectorCaller {
  plan(input: { brief: string }): Promise<FormPlan>;
}

export interface ComposerCaller {
  plan(input: { form: FormPlan; sectionIndex: number }): Promise<SectionPlan>;
}

/// Hierarchical orchestrator. Owns the current FormPlan and tracks
/// progress through the sections. Performer integration is left to
/// the caller (most likely via `DecisionLoop`) — the orchestrator
/// just hands back the SectionPlan for the bar the Performer is in.
export class HierarchicalPlanner {
  private form: FormPlan | null = null;
  private cachedSectionPlans = new Map<number, SectionPlan>();

  constructor(
    private readonly director: DirectorCaller,
    private readonly composer: ComposerCaller,
  ) {}

  hasForm(): boolean {
    return this.form !== null;
  }

  formPlan(): FormPlan | null {
    return this.form;
  }

  async startSong(brief: string): Promise<FormPlan> {
    const form = await this.director.plan({ brief });
    this.form = form;
    this.cachedSectionPlans.clear();
    return form;
  }

  /// Look up which section a given bar belongs to. Returns -1 if
  /// the bar is past the form's end.
  sectionIndexForBar(barIndex: number): number {
    if (!this.form) return -1;
    let cursor = 0;
    for (let i = 0; i < this.form.sections.length; i++) {
      const end = cursor + this.form.sections[i].lengthBars;
      if (barIndex < end) return i;
      cursor = end;
    }
    return -1;
  }

  sectionAtBar(barIndex: number): SectionOutline | null {
    const idx = this.sectionIndexForBar(barIndex);
    if (idx < 0 || !this.form) return null;
    return this.form.sections[idx];
  }

  /// Return (and cache) the SectionPlan for `sectionIndex`. Calls
  /// the Composer on a cache miss. Caller is expected to forward
  /// `plan.setupCalls` to the dispatcher when entering the section.
  async sectionPlan(sectionIndex: number): Promise<SectionPlan> {
    if (!this.form) throw new Error("startSong must be called before sectionPlan");
    if (sectionIndex < 0 || sectionIndex >= this.form.sections.length) {
      throw new Error(`sectionIndex ${sectionIndex} out of range`);
    }
    const cached = this.cachedSectionPlans.get(sectionIndex);
    if (cached) return cached;
    const plan = await this.composer.plan({ form: this.form, sectionIndex });
    this.cachedSectionPlans.set(sectionIndex, plan);
    return plan;
  }

  /// Drop the cached SectionPlan for `sectionIndex`. Useful when the
  /// agent decides to re-think a section after Critic feedback.
  invalidateSectionPlan(sectionIndex: number): void {
    this.cachedSectionPlans.delete(sectionIndex);
  }

  /// Drain all setup calls from sections [from, to] in order. Used by
  /// integration tests + callers that want to apply a span at once.
  async setupCallsForRange(fromSection: number, toSection: number): Promise<ToolCall[]> {
    const out: ToolCall[] = [];
    for (let i = fromSection; i <= toSection; i++) {
      const plan = await this.sectionPlan(i);
      out.push(...plan.setupCalls);
    }
    return out;
  }
}
