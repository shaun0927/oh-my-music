// Pattern variation operators. Built on top of omm-music TS port
// transforms (transposeNotes / invertNotes / retrogradeNotes /
// augmentNotes / humanizeNotes).

import {
  augmentNotes,
  humanizeNotes,
  invertNotes,
  retrogradeNotes,
  transposeNotes,
} from "../music/theory";
import type { Note } from "../music/types";
import type { Pattern } from "./store";

export type VariationStrategy =
  /// Pitch shift by `intervalSemitones`.
  | { kind: "transpose"; intervalSemitones: number }
  /// Mirror pitches around `aroundPitch`.
  | { kind: "invert"; aroundPitch: number }
  /// Reverse temporal order.
  | { kind: "retrograde" }
  /// Stretch/compress timing by `factor`.
  | { kind: "augment"; factor: number }
  /// Add bounded random jitter to velocity + start position. Seed
  /// makes it reproducible.
  | {
      kind: "humanize";
      velocityJitter: number;
      timingJitterTicks: number;
      seed: number;
    }
  /// Composite: apply multiple variations in order.
  | { kind: "composite"; steps: VariationStrategy[] };

export function varyPattern(
  source: Pattern,
  strategy: VariationStrategy,
  newId: string,
  options: { newName?: string; addTags?: string[] } = {},
): Pattern {
  const newNotes = applyVariation(source.notes, strategy);
  return {
    ...source,
    id: newId,
    name: options.newName ?? `${source.name} ʹ`,
    notes: newNotes,
    tags: dedupeTags([...source.tags, "variation", ...(options.addTags ?? [])]),
    createdAt: new Date().toISOString(),
  };
}

function applyVariation(notes: Note[], strategy: VariationStrategy): Note[] {
  const clone = notes.map((n) => ({ ...n }));
  switch (strategy.kind) {
    case "transpose":
      return transposeNotes(clone, strategy.intervalSemitones);
    case "invert":
      return invertNotes(clone, strategy.aroundPitch);
    case "retrograde":
      return retrogradeNotes(clone);
    case "augment":
      return augmentNotes(clone, strategy.factor);
    case "humanize":
      return humanizeNotes(
        clone,
        strategy.velocityJitter,
        strategy.timingJitterTicks,
        strategy.seed,
      );
    case "composite": {
      let acc = clone;
      for (const step of strategy.steps) {
        acc = applyVariation(acc, step);
      }
      return acc;
    }
  }
}

function dedupeTags(tags: string[]): string[] {
  return Array.from(new Set(tags));
}
