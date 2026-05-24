// User profile + decision log store (Epic #1 E5).
//
// Backed by bun:sqlite (in-memory default, accepts a path for
// persistence). Schema mirrors `docs/ARCHITECTURE.md` §9.6 in
// trimmed form for the current scope.

import { Database } from "bun:sqlite";

export type ProfileScope = "local-user" | "discord-user" | "guild";

export interface Profile {
  id: string;
  scope: ProfileScope;
  discordUserId?: string | null;
  discordGuildId?: string | null;
  preferredEnergy: number;
  volumeLimitDb: number;
  favoriteStyles: string[];
  createdAt: string;
  updatedAt: string;
}

export interface DecisionLogEntry {
  sessionId: string;
  createdAt: string;
  contextJson: string;
  toolCallsJson: string;
  rationale: string | null;
}

const SCHEMA = `
CREATE TABLE IF NOT EXISTS profiles (
  id                TEXT PRIMARY KEY,
  scope             TEXT NOT NULL,
  discord_user_id   TEXT,
  discord_guild_id  TEXT,
  preferred_energy  REAL DEFAULT 0.5,
  volume_limit_db   REAL DEFAULT -1.0,
  favorite_styles   TEXT DEFAULT '[]',
  created_at        TEXT NOT NULL,
  updated_at        TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS decisions (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id      TEXT NOT NULL,
  created_at      TEXT NOT NULL,
  context_json    TEXT NOT NULL,
  tool_calls_json TEXT NOT NULL,
  rationale       TEXT
);
CREATE INDEX IF NOT EXISTS idx_decisions_session ON decisions(session_id);
`;

interface ProfileRow {
  id: string;
  scope: string;
  discord_user_id: string | null;
  discord_guild_id: string | null;
  preferred_energy: number;
  volume_limit_db: number;
  favorite_styles: string;
  created_at: string;
  updated_at: string;
}

interface DecisionRow {
  session_id: string;
  created_at: string;
  context_json: string;
  tool_calls_json: string;
  rationale: string | null;
}

export class ProfileStore {
  private db: Database;

  constructor(path: string = ":memory:") {
    this.db = new Database(path);
    this.db.exec(SCHEMA);
  }

  close(): void {
    this.db.close();
  }

  upsertProfile(profile: Omit<Profile, "createdAt" | "updatedAt"> & {
    createdAt?: string;
    updatedAt?: string;
  }): Profile {
    const existing = this.getProfile(profile.id);
    const now = new Date().toISOString();
    const createdAt = existing?.createdAt ?? profile.createdAt ?? now;
    const updatedAt = profile.updatedAt ?? now;
    this.db
      .prepare(
        `INSERT OR REPLACE INTO profiles
         (id, scope, discord_user_id, discord_guild_id,
          preferred_energy, volume_limit_db, favorite_styles,
          created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        profile.id,
        profile.scope,
        profile.discordUserId ?? null,
        profile.discordGuildId ?? null,
        profile.preferredEnergy,
        profile.volumeLimitDb,
        JSON.stringify(profile.favoriteStyles ?? []),
        createdAt,
        updatedAt,
      );
    return this.getProfile(profile.id)!;
  }

  getProfile(id: string): Profile | null {
    const row = this.db.prepare(`SELECT * FROM profiles WHERE id = ?`).get(id) as
      | ProfileRow
      | undefined;
    return row ? rowToProfile(row) : null;
  }

  listProfiles(scope?: ProfileScope): Profile[] {
    const rows = scope
      ? (this.db.prepare(`SELECT * FROM profiles WHERE scope = ?`).all(scope) as ProfileRow[])
      : (this.db.prepare(`SELECT * FROM profiles ORDER BY created_at`).all() as ProfileRow[]);
    return rows.map(rowToProfile);
  }

  removeProfile(id: string): boolean {
    return this.db.prepare(`DELETE FROM profiles WHERE id = ?`).run(id).changes > 0;
  }

  appendDecision(entry: DecisionLogEntry): void {
    this.db
      .prepare(
        `INSERT INTO decisions (session_id, created_at, context_json, tool_calls_json, rationale)
         VALUES (?, ?, ?, ?, ?)`,
      )
      .run(
        entry.sessionId,
        entry.createdAt,
        entry.contextJson,
        entry.toolCallsJson,
        entry.rationale,
      );
  }

  recentDecisions(sessionId: string, limit = 10): DecisionLogEntry[] {
    const rows = this.db
      .prepare(
        `SELECT session_id, created_at, context_json, tool_calls_json, rationale
         FROM decisions WHERE session_id = ?
         ORDER BY id DESC LIMIT ?`,
      )
      .all(sessionId, limit) as DecisionRow[];
    return rows.reverse().map(rowToDecision);
  }

  decisionCount(sessionId?: string): number {
    const row = sessionId
      ? (this.db
          .prepare(`SELECT COUNT(*) AS n FROM decisions WHERE session_id = ?`)
          .get(sessionId) as { n: number })
      : (this.db.prepare(`SELECT COUNT(*) AS n FROM decisions`).get() as { n: number });
    return row.n;
  }
}

function rowToProfile(row: ProfileRow): Profile {
  return {
    id: row.id,
    scope: row.scope as ProfileScope,
    discordUserId: row.discord_user_id ?? undefined,
    discordGuildId: row.discord_guild_id ?? undefined,
    preferredEnergy: row.preferred_energy,
    volumeLimitDb: row.volume_limit_db,
    favoriteStyles: JSON.parse(row.favorite_styles ?? "[]"),
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function rowToDecision(row: DecisionRow): DecisionLogEntry {
  return {
    sessionId: row.session_id,
    createdAt: row.created_at,
    contextJson: row.context_json,
    toolCallsJson: row.tool_calls_json,
    rationale: row.rationale,
  };
}
