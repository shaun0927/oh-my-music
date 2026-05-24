// Bun entry point. Three modes:
//
//   bun run src/index.ts                # boot wired stack with noop LLM
//   bun run src/index.ts pi-session     # back-compat: Pi-SDK session only
//   bun run src/index.ts --socket=/path # boot wired stack against given UDS
//
// The "wired stack" mode (default) sets up PatternStore + ProfileStore +
// musicTools + DecisionLoop with the noop LLM. It connects to the
// engine over UDS only if --socket is passed.

import { createMusicAgent } from "./agent/decision";
import { noopLlmCaller, wireAgent } from "./wire";

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("pi-session")) {
    const agent = await createMusicAgent();
    console.log(`oh-my-music agent ready (pi-session): ${agent.sessionId}`);
    return;
  }

  const socketArg = args.find((a) => a.startsWith("--socket="));
  const socketPath = socketArg?.split("=", 2)[1];

  const wired = await wireAgent(noopLlmCaller, { socketPath });
  console.log(
    `oh-my-music agent wired. tools=${wired.tools.length} socket=${socketPath ?? "(none)"}`,
  );
  // Run one immediate cycle so the boot output is non-trivial.
  const report = await wired.decisionLoop.tick();
  if (report) {
    console.log(
      `cycle: accepted=${report.accepted.length} rejected=${report.rejected.length} truncated=${report.truncated.length}`,
    );
  }
  await wired.shutdown();
}

await main();
