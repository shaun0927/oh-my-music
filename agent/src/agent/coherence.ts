// Long-form coherence helpers (Epic #1 Phase 8 / D5).
//
// Tracks motif callbacks ("intro pad recurs in bridge a minor third
// lower") and the actual energy curve vs the Director's planned
// curve so the agent can spot drift early.

import type { FormPlan } from "./form";

export interface MotifCallback {
  patternId: string;
  /// Section the original was first established in.
  originSectionIndex: number;
  /// Section where the motif returns.
  recurrenceSectionIndex: number;
  /// Transformation applied (free-text: "transpose -3", "retrograde", …).
  transformation: string;
  /// True when the callback has actually been heard in the rendered
  /// audio (vs merely planned).
  heard: boolean;
}

export class MotifRegistry {
  private callbacks: MotifCallback[] = [];

  /// Register an intended motif return ahead of time.
  planCallback(callback: Omit<MotifCallback, "heard">): void {
    this.callbacks.push({ ...callback, heard: false });
  }

  /// Mark every planned callback whose recurrenceSectionIndex matches
  /// `sectionIndex` as "heard".
  markHeard(sectionIndex: number): MotifCallback[] {
    const out: MotifCallback[] = [];
    for (const c of this.callbacks) {
      if (c.recurrenceSectionIndex === sectionIndex && !c.heard) {
        c.heard = true;
        out.push(c);
      }
    }
    return out;
  }

  all(): MotifCallback[] {
    return this.callbacks.slice();
  }

  /// Callbacks that were planned but the recurrence section has passed
  /// without being marked as heard.
  missed(currentSectionIndex: number): MotifCallback[] {
    return this.callbacks.filter(
      (c) => !c.heard && c.recurrenceSectionIndex < currentSectionIndex,
    );
  }
}

export interface TensionSample {
  sectionIndex: number;
  /// Director's planned energy at this point (0..1).
  plannedEnergy: number;
  /// What the renderer actually produced (0..1, derived from RMS).
  actualEnergy: number;
}

export class TensionTracker {
  private samples: TensionSample[] = [];

  recordSection(sectionIndex: number, plannedEnergy: number, actualEnergy: number): void {
    this.samples.push({ sectionIndex, plannedEnergy, actualEnergy });
  }

  samplesView(): TensionSample[] {
    return this.samples.slice();
  }

  /// Greatest abs(actual − planned) across the song so far. A large
  /// number means the Composer is drifting off the Director's plan.
  maxDrift(): number {
    let max = 0;
    for (const s of this.samples) {
      max = Math.max(max, Math.abs(s.actualEnergy - s.plannedEnergy));
    }
    return max;
  }

  /// Compute a one-line summary the Director can read between sections.
  summary(form: FormPlan): string {
    if (this.samples.length === 0) return "no samples yet";
    const drift = this.maxDrift();
    const last = this.samples[this.samples.length - 1];
    const sectionName = form.sections[last.sectionIndex]?.name ?? `section ${last.sectionIndex}`;
    return `last: ${sectionName} planned=${last.plannedEnergy.toFixed(2)} actual=${last.actualEnergy.toFixed(2)} | maxDrift=${drift.toFixed(2)}`;
  }
}

/// Convert a deterministic verdict's RMS into a 0..1 energy proxy.
/// Crude: maps −40 dB → 0, −6 dB → 1, clamps elsewhere.
export function rmsDbToEnergy(rmsDb: number): number {
  const lo = -40;
  const hi = -6;
  return Math.max(0, Math.min(1, (rmsDb - lo) / (hi - lo)));
}
