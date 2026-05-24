import { describe, expect, test } from "bun:test";

import {
  buildUserInputFromSlashCommand,
  DISCORD_COMMAND_SPECS,
  parseMentionMessage,
} from "../commands";
import { createDiscordBot } from "../bot";
import { DecisionLoop, type LlmCaller, type ToolDispatcher } from "../../../agent/decision";
import { defaultLlmContext } from "../../../agent/context";

function makeLoop(): {
  loop: DecisionLoop;
  received: (string | null)[];
} {
  const received: (string | null)[] = [];
  const caller: LlmCaller = {
    async prompt(input) {
      received.push(input.userInput);
      return [];
    },
  };
  const dispatcher: ToolDispatcher = {
    async dispatch() {
      return "ok";
    },
  };
  const loop = new DecisionLoop(caller, dispatcher, () => defaultLlmContext("discord"));
  return { loop, received };
}

describe("DISCORD_COMMAND_SPECS", () => {
  test("includes all six expected commands", () => {
    const names = DISCORD_COMMAND_SPECS.map((s) => s.name);
    expect(names).toEqual(["omm-start", "omm-stop", "omm-mood", "omm-play", "omm-duck", "omm-energy"]);
  });

  test("omm-mood choices match the seven mood literals", () => {
    const moodSpec = DISCORD_COMMAND_SPECS.find((s) => s.name === "omm-mood")!;
    expect(moodSpec.options[0].choices).toEqual([
      "calm",
      "focus",
      "energetic",
      "dark",
      "bright",
      "dreamy",
      "minimal",
    ]);
  });
});

describe("buildUserInputFromSlashCommand", () => {
  test("omm-start sets shouldJoin", () => {
    const p = buildUserInputFromSlashCommand("omm-start", {});
    expect(p.shouldJoin).toBe(true);
    expect(p.shouldLeave).toBe(false);
  });

  test("omm-stop sets shouldLeave", () => {
    const p = buildUserInputFromSlashCommand("omm-stop", {});
    expect(p.shouldLeave).toBe(true);
    expect(p.shouldJoin).toBe(false);
  });

  test("omm-mood renders the mood into userInput", () => {
    const p = buildUserInputFromSlashCommand("omm-mood", { mood: "calm" });
    expect(p.userInput).toContain("calm");
  });

  test("omm-play includes the url", () => {
    const p = buildUserInputFromSlashCommand("omm-play", { url: "/tmp/x.mp3" });
    expect(p.userInput).toContain("/tmp/x.mp3");
  });
});

describe("parseMentionMessage", () => {
  test("strips the <@id> prefix and returns the remaining text", () => {
    const got = parseMentionMessage("<@12345> make it warmer", "12345");
    expect(got?.userInput).toBe("make it warmer");
  });

  test("handles the <@!id> nickname-mention variant", () => {
    const got = parseMentionMessage("<@!12345> calm down", "12345");
    expect(got?.userInput).toBe("calm down");
  });

  test("returns null when the message doesn't mention the bot", () => {
    expect(parseMentionMessage("hello chat", "12345")).toBeNull();
  });

  test("returns null when the mention has nothing after it", () => {
    expect(parseMentionMessage("<@12345>", "12345")).toBeNull();
  });
});

describe("DiscordBot", () => {
  test("slash command enqueues user input into DecisionLoop and calls ack sink", async () => {
    const { loop, received } = makeLoop();
    let acked: string | undefined;
    const bot = createDiscordBot({
      botUserId: "bot",
      loop,
      sink: {
        async onAck(text) {
          acked = text;
        },
      },
    });
    await bot.start();
    const text = await bot.handleSlashCommand("omm-mood", { mood: "focus" });
    expect(text).toContain("focus");
    expect(acked).toContain("focus");
    await loop.tick();
    expect(received[0]).toContain("focus");
  });

  test("omm-start triggers onJoinRequested", async () => {
    const { loop } = makeLoop();
    let joined = false;
    const bot = createDiscordBot({
      botUserId: "bot",
      loop,
      sink: { async onJoinRequested() { joined = true; } },
    });
    await bot.start();
    await bot.handleSlashCommand("omm-start", {});
    expect(joined).toBe(true);
  });

  test("@mention chat message routes to loop", async () => {
    const { loop, received } = makeLoop();
    const bot = createDiscordBot({ botUserId: "42", loop });
    await bot.start();
    const result = await bot.handleMessage("<@42> 좀 더 차분하게");
    expect(result).toBe("좀 더 차분하게");
    await loop.tick();
    expect(received[0]).toBe("좀 더 차분하게");
  });

  test("handleSlashCommand throws if start() wasn't called", async () => {
    const { loop } = makeLoop();
    const bot = createDiscordBot({ botUserId: "bot", loop });
    await expect(bot.handleSlashCommand("omm-mood", { mood: "calm" })).rejects.toThrow(/not started/);
  });
});
