import { Type } from "@mariozechner/pi-ai";
import { defineTool } from "@mariozechner/pi-coding-agent";

import type { EngineClient } from "../../engine-client";

const TIME_SIG = Type.Object({
  numerator: Type.Number({ minimum: 1, maximum: 32 }),
  denominator: Type.Union([
    Type.Literal(1),
    Type.Literal(2),
    Type.Literal(4),
    Type.Literal(8),
    Type.Literal(16),
    Type.Literal(32),
  ]),
});

export function setTransportTool(engineClient: EngineClient) {
  return defineTool({
    name: "set_transport",
    label: "Set Transport",
    description:
      "Replace the engine's master musical transport (BPM, time signature, swing). Effective from the current engine frame; downstream sequencers reset their start frame.",
    parameters: Type.Object({
      bpm: Type.Number({ minimum: 30, maximum: 300 }),
      timeSignature: TIME_SIG,
      swing: Type.Optional(Type.Number({ minimum: 0, maximum: 1 })),
    }),
    executionMode: "sequential",
    execute: async (_id, params) => {
      await engineClient.sendParamBatch({
        type: "set_transport",
        transport: {
          bpm: params.bpm,
          time_signature: params.timeSignature,
          swing: params.swing ?? 0,
        },
      });
      return {
        content: [
          {
            type: "text",
            text: `Transport set to ${params.bpm} BPM ${params.timeSignature.numerator}/${params.timeSignature.denominator}`,
          },
        ],
        details: params,
      };
    },
  });
}
