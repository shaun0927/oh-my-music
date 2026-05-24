// Pure formatting helpers for the TUI. Kept in a separate module so
// the unit tests don't need Ink / React rendering machinery.

import type { LlmContext, RecentDecision } from "../../agent/context";

/// Render a single horizontal level meter (ASCII bar) for a dB value.
/// `db` ≤ `floor` collapses to empty; `db` ≥ 0 saturates the bar.
export function renderMeterBar(db: number, width: number = 14, floor: number = -60): string {
  const clamped = Math.max(floor, Math.min(0, db));
  const filled = Math.round(((clamped - floor) / -floor) * width);
  return "█".repeat(filled) + "░".repeat(Math.max(0, width - filled));
}

/// Format `dB` as a 6-char fixed-width string, e.g. " -12.3" or "  0.0".
export function formatDb(db: number): string {
  const s = db.toFixed(1);
  return s.padStart(6, " ");
}

/// One-line summary of the current state for the TUI header strip.
export function statusLine(context: LlmContext): string {
  const s = context.currentState;
  const env = context.environment;
  const bpm = s.bpm ?? "—";
  const key = s.key ?? "—";
  return `BPM ${bpm}  Key ${key}  Env ${env.label}  Energy ${s.energy.toFixed(2)}`;
}

/// Render the recent-decisions log as ASCII lines (latest first).
export function decisionsToLines(decisions: RecentDecision[], limit: number = 5): string[] {
  return decisions
    .slice(-limit)
    .reverse()
    .map((d) => `  ${shortTime(d.at)}  ${d.summary}`);
}

function shortTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const hh = d.getHours().toString().padStart(2, "0");
  const mm = d.getMinutes().toString().padStart(2, "0");
  const ss = d.getSeconds().toString().padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

export interface SourceMeterRow {
  label: string;
  db: number;
  flags?: string; // "muted", "solo", etc.
  ornament?: string; // e.g. "♫ lo-fi.mp3"
}

/// Render a per-source meter row that lines up with `renderMeterBar`.
/// Used by the Sources panel: `| label | bar | db | ornament |`.
export function formatSourceRow(row: SourceMeterRow, labelWidth: number = 10): string {
  const label = row.label.padEnd(labelWidth, " ").slice(0, labelWidth);
  const bar = renderMeterBar(row.db);
  const db = formatDb(row.db);
  const flags = row.flags ? `[${row.flags}]` : "";
  const ornament = row.ornament ?? "";
  return [label, bar, `${db} dB`, flags, ornament].filter(Boolean).join("  ").trimEnd();
}
