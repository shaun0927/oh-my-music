// Song-level form types used by the Director → Composer → Performer
// hierarchy (Epic #1 Phase 5 / D2).

import type { ToolCall } from "./safety";

export type SectionRole =
  | "intro"
  | "verse"
  | "chorus"
  | "bridge"
  | "drop"
  | "breakdown"
  | "outro";

export interface SectionOutline {
  /// Display name (caller-chosen; Director may produce things like
  /// "intro / pad bed" or "first chorus").
  name: string;
  role: SectionRole;
  lengthBars: number;
  /// Motif/pattern IDs (from `PatternStore`) the Composer should
  /// foreground in this section. Director can also leave this empty
  /// and let the Composer pick.
  preferredPatternIds: string[];
  /// Director's hint about energy / tension at this point on [0, 1].
  targetEnergy: number;
  /// Free-text comment for the LLM ("call back the chorus motif a
  /// minor third lower"). Performer logs it for context.
  comment?: string;
}

export interface FormPlan {
  /// Free-text song-level brief (the user's directive, paraphrased).
  brief: string;
  /// Total intended song length, used by the Composer to budget
  /// transitions.
  totalBars: number;
  sections: SectionOutline[];
  /// Bar index where the climax peaks. 0 = no explicit climax.
  climaxBarIndex: number;
}

export interface SectionPlan {
  /// Identifies which `FormPlan.sections[i]` this plan elaborates.
  sectionIndex: number;
  /// One-line summary the Performer sees as context.
  summary: string;
  /// Tool calls the Performer should issue at the start of this
  /// section (e.g. switch transport, swap chord pattern). Per-cycle
  /// fine adjustments still come from the Performer itself.
  setupCalls: ToolCall[];
}
