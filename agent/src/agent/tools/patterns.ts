// Pattern Memory LLM tools. The agent can save a motif under a name,
// recall it later, ask for a variation, and list everything that's in
// scope. Backed by `PatternStore` in `agent/src/pattern/store.ts`.

import { Type } from "@mariozechner/pi-ai";
import { defineTool } from "@mariozechner/pi-coding-agent";

import type { PatternStore } from "../../pattern/store";
import { varyPattern } from "../../pattern/variation";

const ROLE = Type.Union([
  Type.Literal("intro"),
  Type.Literal("verse"),
  Type.Literal("chorus"),
  Type.Literal("bridge"),
  Type.Literal("motif"),
  Type.Literal("fill"),
  Type.Literal("outro"),
]);

const NOTE = Type.Object({
  pitch: Type.Number({ minimum: 0, maximum: 127 }),
  velocity: Type.Number({ minimum: 0, maximum: 127 }),
  start_ticks: Type.Number({ minimum: 0 }),
  length_ticks: Type.Number({ minimum: 1 }),
});

const VARIATION = Type.Union([
  Type.Object({
    kind: Type.Literal("transpose"),
    intervalSemitones: Type.Number({ minimum: -24, maximum: 24 }),
  }),
  Type.Object({
    kind: Type.Literal("invert"),
    aroundPitch: Type.Number({ minimum: 0, maximum: 127 }),
  }),
  Type.Object({ kind: Type.Literal("retrograde") }),
  Type.Object({
    kind: Type.Literal("augment"),
    factor: Type.Number({ minimum: 0.25, maximum: 4 }),
  }),
  Type.Object({
    kind: Type.Literal("humanize"),
    velocityJitter: Type.Number({ minimum: 0, maximum: 64 }),
    timingJitterTicks: Type.Number({ minimum: 0, maximum: 480 }),
    seed: Type.Number({ minimum: 0 }),
  }),
]);

export function savePatternTool(store: PatternStore) {
  return defineTool({
    name: "save_pattern",
    label: "Save Pattern",
    description:
      "Persist a named pattern (motif / verse / chorus / etc) so future cycles can recall and vary it.",
    parameters: Type.Object({
      id: Type.String(),
      name: Type.String(),
      role: ROLE,
      notes: Type.Array(NOTE),
      voicing: Type.Optional(Type.Array(Type.Number({ minimum: 0, maximum: 127 }))),
      lengthBars: Type.Number({ minimum: 1, maximum: 64 }),
      tags: Type.Optional(Type.Array(Type.String())),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      store.save({
        id: params.id,
        name: params.name,
        role: params.role,
        notes: params.notes,
        voicing: params.voicing,
        lengthBars: params.lengthBars,
        tags: params.tags ?? [],
        createdAt: new Date().toISOString(),
      });
      return {
        content: [{ type: "text", text: `Saved pattern ${params.id}.` }],
        details: { id: params.id },
      };
    },
  });
}

export function recallPatternTool(store: PatternStore) {
  return defineTool({
    name: "recall_pattern",
    label: "Recall Pattern",
    description: "Fetch a previously-saved pattern by id.",
    parameters: Type.Object({ id: Type.String() }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const pattern = store.recall(params.id);
      if (!pattern) {
        return {
          content: [{ type: "text", text: `No pattern with id ${params.id}.` }],
          details: { found: false, pattern: null as any },
        };
      }
      return {
        content: [
          {
            type: "text",
            text: `Pattern ${pattern.id} (${pattern.role}, ${pattern.notes.length} notes).`,
          },
        ],
        details: { found: true, pattern },
      };
    },
  });
}

export function varyPatternTool(store: PatternStore) {
  return defineTool({
    name: "vary_pattern",
    label: "Vary Pattern",
    description:
      "Create a derived pattern by transforming an existing one (transpose / invert / retrograde / augment / humanize).",
    parameters: Type.Object({
      sourceId: Type.String(),
      newId: Type.String(),
      strategy: VARIATION,
      newName: Type.Optional(Type.String()),
      addTags: Type.Optional(Type.Array(Type.String())),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const source = store.recall(params.sourceId);
      if (!source) {
        return {
          content: [{ type: "text", text: `Source pattern ${params.sourceId} not found.` }],
          details: { ok: false, newId: null as any },
        };
      }
      const varied = varyPattern(source, params.strategy as any, params.newId, {
        newName: params.newName,
        addTags: params.addTags,
      });
      store.save(varied);
      return {
        content: [{ type: "text", text: `Saved variation ${varied.id} (${params.strategy.kind}).` }],
        details: { ok: true, newId: varied.id },
      };
    },
  });
}

export function listPatternsTool(store: PatternStore) {
  return defineTool({
    name: "list_patterns",
    label: "List Patterns",
    description: "List all saved patterns (optionally filtered by role or tag).",
    parameters: Type.Object({
      role: Type.Optional(ROLE),
      tag: Type.Optional(Type.String()),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      const filter = (() => {
        if (params.role && params.tag) return { role: params.role, tag: params.tag };
        if (params.role) return { role: params.role };
        if (params.tag) return { tag: params.tag };
        return undefined;
      })();
      const patterns = store.list(filter);
      return {
        content: [{ type: "text", text: `${patterns.length} pattern(s).` }],
        details: { patterns: patterns.map((p) => ({ id: p.id, role: p.role, name: p.name })) },
      };
    },
  });
}

export function patternTools(store: PatternStore) {
  return [
    savePatternTool(store),
    recallPatternTool(store),
    varyPatternTool(store),
    listPatternsTool(store),
  ];
}
