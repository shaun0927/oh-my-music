import type { EngineClient } from "../engine-client";
import { PatternStore } from "../pattern/store";
import { createPatternTool } from "./tools/glicol";
import { setEnergyTool, setMoodTool } from "./tools/energy";
import { patternTools } from "./tools/patterns";
import { sequencerTools } from "./tools/sequencer";
import { theoryTools } from "./tools/theory";
import { setTransportTool } from "./tools/transport";
import { emergencyFadeTool, resetMixTool } from "./tools/utility";

/// Pass `patternStore` to use a shared (typically persistent) store
/// across cycles. When omitted, an in-memory PatternStore is created
/// for the lifetime of this `musicTools` call (callers will lose
/// memory between sessions).
export function musicTools(engineClient: EngineClient, patternStore?: PatternStore) {
  const store = patternStore ?? new PatternStore();
  return [
    setEnergyTool(engineClient),
    setMoodTool(engineClient),
    createPatternTool(engineClient),
    emergencyFadeTool(engineClient),
    resetMixTool(engineClient),
    setTransportTool(engineClient),
    ...sequencerTools(engineClient),
    ...theoryTools(),
    ...patternTools(store),
  ];
}
