import { Type } from "@mariozechner/pi-ai";
import { defineTool } from "@mariozechner/pi-coding-agent";

import type { EngineClient } from "../../engine-client";

const NOTE = Type.Object({
  pitch_midi: Type.Number({ minimum: 0, maximum: 127 }),
  velocity: Type.Number({ minimum: 0, maximum: 127 }),
  start: Type.Object({
    bar: Type.Number({ minimum: 0 }),
    beat: Type.Number({ minimum: 0, maximum: 31 }),
    tick: Type.Number({ minimum: 0, maximum: 479 }),
  }),
  length_ticks: Type.Number({ minimum: 1, maximum: 480 * 32 }),
  channel: Type.Number({ minimum: 0, maximum: 15 }),
});

const TRIGGER = Type.Union([
  Type.Object({ kind: Type.Literal("frame"), frame: Type.Number({ minimum: 0 }) }),
  Type.Object({
    kind: Type.Literal("musical"),
    bar: Type.Number({ minimum: 0 }),
    beat: Type.Number({ minimum: 0 }),
    tick: Type.Number({ minimum: 0 }),
  }),
  Type.Object({
    kind: Type.Literal("relative"),
    bars: Type.Number({ minimum: 0 }),
    beats: Type.Number({ minimum: 0 }),
  }),
  Type.Object({ kind: Type.Literal("nextBar") }),
  Type.Object({ kind: Type.Literal("nextBeat") }),
]);

export function createSequencerSourceTool(engineClient: EngineClient) {
  return defineTool({
    name: "create_sequencer_source",
    label: "Create Sequencer Source",
    description:
      "Spin up a new note-event-driven generated source backed by a polyphonic synth voice pool.",
    parameters: Type.Object({
      sourceInstanceId: Type.String(),
      voiceType: Type.Union([Type.Literal("SineAdsr"), Type.Literal("SawAdsr")]),
      polyphony: Type.Number({ minimum: 1, maximum: 32 }),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      await engineClient.sendParamBatch({ type: "create_sequencer_source", ...params });
      return {
        content: [{ type: "text", text: `Sequencer source ${params.sourceInstanceId} created.` }],
        details: params,
      };
    },
  });
}

export function removeSequencerSourceTool(engineClient: EngineClient) {
  return defineTool({
    name: "remove_sequencer_source",
    label: "Remove Sequencer Source",
    description: "Stop and tear down a previously-created sequencer source with an optional fade.",
    parameters: Type.Object({
      sourceInstanceId: Type.String(),
      fadeMs: Type.Number({ minimum: 0, maximum: 5000 }),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      await engineClient.sendParamBatch({ type: "remove_sequencer_source", ...params });
      return {
        content: [{ type: "text", text: `Sequencer source ${params.sourceInstanceId} removed.` }],
        details: params,
      };
    },
  });
}

export function scheduleNotesTool(engineClient: EngineClient) {
  return defineTool({
    name: "schedule_notes",
    label: "Schedule Note Batch",
    description:
      "Schedule a batch of MIDI-style note events on a sequencer source. The trigger expresses when to start (next bar / N bars from now / specific musical time / absolute frame). Planned-LLM origin requires a trigger >= 30 seconds in the future.",
    parameters: Type.Object({
      sourceInstanceId: Type.String(),
      notes: Type.Array(NOTE),
      trigger: TRIGGER,
      loopBars: Type.Optional(Type.Number({ minimum: 1, maximum: 64 })),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      await engineClient.sendParamBatch({ type: "schedule_notes", ...params });
      return {
        content: [
          {
            type: "text",
            text: `Scheduled ${params.notes.length} notes on ${params.sourceInstanceId}.`,
          },
        ],
        details: params,
      };
    },
  });
}

export function clearNotesTool(engineClient: EngineClient) {
  return defineTool({
    name: "clear_notes",
    label: "Clear Pending Notes",
    description: "Cancel pending notes on a sequencer source (optionally from a musical time onward).",
    parameters: Type.Object({
      sourceInstanceId: Type.String(),
      fromBar: Type.Optional(Type.Number({ minimum: 0 })),
      fromBeat: Type.Optional(Type.Number({ minimum: 0 })),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      await engineClient.sendParamBatch({ type: "clear_notes", ...params });
      return {
        content: [{ type: "text", text: `Pending notes cleared on ${params.sourceInstanceId}.` }],
        details: params,
      };
    },
  });
}

export function sequencerTools(engineClient: EngineClient) {
  return [
    createSequencerSourceTool(engineClient),
    removeSequencerSourceTool(engineClient),
    scheduleNotesTool(engineClient),
    clearNotesTool(engineClient),
  ];
}
