// Pattern Memory & Motif store (Epic #1 Phase 4 / D1).
//
// Stores named musical patterns the agent can recall, vary, and
// schedule across decision cycles. Backed by bun:sqlite (in-memory by
// default; pass a path to persist).

import { Database } from "bun:sqlite";

import type { Note } from "../music/types";

export type PatternRole =
  | "intro"
  | "verse"
  | "chorus"
  | "bridge"
  | "motif"
  | "fill"
  | "outro";

export interface Pattern {
  id: string;
  name: string;
  role: PatternRole;
  notes: Note[];
  voicing?: number[]; // optional pitch list (root-position voicing)
  lengthBars: number;
  tags: string[];
  createdAt: string; // ISO 8601
}

interface RowRecord {
  id: string;
  name: string;
  role: string;
  notes: string;
  voicing: string | null;
  length_bars: number;
  tags: string;
  created_at: string;
}

const SCHEMA = `
CREATE TABLE IF NOT EXISTS patterns (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  role        TEXT NOT NULL,
  notes       TEXT NOT NULL,
  voicing     TEXT,
  length_bars INTEGER NOT NULL,
  tags        TEXT NOT NULL,
  created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_patterns_role ON patterns(role);
`;

export class PatternStore {
  private db: Database;

  constructor(path: string = ":memory:") {
    this.db = new Database(path);
    this.db.exec(SCHEMA);
  }

  close(): void {
    this.db.close();
  }

  save(pattern: Pattern): void {
    this.db
      .prepare(
        `INSERT OR REPLACE INTO patterns
         (id, name, role, notes, voicing, length_bars, tags, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        pattern.id,
        pattern.name,
        pattern.role,
        JSON.stringify(pattern.notes),
        pattern.voicing ? JSON.stringify(pattern.voicing) : null,
        pattern.lengthBars,
        JSON.stringify(pattern.tags),
        pattern.createdAt,
      );
  }

  recall(id: string): Pattern | null {
    const row = this.db.prepare(`SELECT * FROM patterns WHERE id = ?`).get(id) as
      | RowRecord
      | undefined;
    return row ? rowToPattern(row) : null;
  }

  list(filter?: { role?: PatternRole; tag?: string }): Pattern[] {
    let rows: RowRecord[];
    if (filter?.role) {
      rows = this.db
        .prepare(`SELECT * FROM patterns WHERE role = ? ORDER BY created_at`)
        .all(filter.role) as RowRecord[];
    } else {
      rows = this.db.prepare(`SELECT * FROM patterns ORDER BY created_at`).all() as RowRecord[];
    }
    const patterns = rows.map(rowToPattern);
    if (filter?.tag) {
      return patterns.filter((p) => p.tags.includes(filter.tag!));
    }
    return patterns;
  }

  remove(id: string): boolean {
    const result = this.db.prepare(`DELETE FROM patterns WHERE id = ?`).run(id);
    return result.changes > 0;
  }

  /// Lightweight summary the LLM context builder injects so the agent
  /// always knows what motifs are in scope.
  summary(): { id: string; name: string; role: PatternRole; lengthBars: number; tags: string[] }[] {
    return this.list().map((p) => ({
      id: p.id,
      name: p.name,
      role: p.role,
      lengthBars: p.lengthBars,
      tags: p.tags,
    }));
  }

  count(): number {
    const row = this.db.prepare(`SELECT COUNT(*) AS n FROM patterns`).get() as { n: number };
    return row.n;
  }
}

function rowToPattern(row: RowRecord): Pattern {
  return {
    id: row.id,
    name: row.name,
    role: row.role as PatternRole,
    notes: JSON.parse(row.notes) as Note[],
    voicing: row.voicing ? (JSON.parse(row.voicing) as number[]) : undefined,
    lengthBars: row.length_bars,
    tags: JSON.parse(row.tags) as string[],
    createdAt: row.created_at,
  };
}
