import type { EngineClient } from "../engine-client";
import { createPatternTool } from "./tools/glicol";
import { setEnergyTool, setMoodTool } from "./tools/energy";
import { sequencerTools } from "./tools/sequencer";
import { theoryTools } from "./tools/theory";
import { setTransportTool } from "./tools/transport";
import { emergencyFadeTool, resetMixTool } from "./tools/utility";

export function musicTools(engineClient: EngineClient) {
  return [
    setEnergyTool(engineClient),
    setMoodTool(engineClient),
    createPatternTool(engineClient),
    emergencyFadeTool(engineClient),
    resetMixTool(engineClient),
    setTransportTool(engineClient),
    ...sequencerTools(engineClient),
    ...theoryTools(),
  ];
}
