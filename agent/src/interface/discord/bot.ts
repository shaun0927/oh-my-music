// Discord bot adapter. Wraps the pure command-parsing layer in
// `commands.ts` with a thin discord.js client. Network code is kept
// behind an interface so unit tests can stub the gateway entirely.

import type { DecisionLoop } from "../../agent/decision";
import {
  buildUserInputFromSlashCommand,
  DISCORD_COMMAND_SPECS,
  parseMentionMessage,
  type DiscordCommandName,
} from "./commands";

export interface DiscordEventSink {
  /// Called when /omm-start fires (after parser determines a join is
  /// requested). The host process should join the user's voice
  /// channel here; we keep the side effect outside the bot core for
  /// testability.
  onJoinRequested?: () => Promise<void> | void;
  onLeaveRequested?: () => Promise<void> | void;
  /// Called once per ack-able interaction so the host can post a
  /// reply / typing indicator. The bot passes through whatever
  /// short ack text it generated.
  onAck?: (text: string) => Promise<void> | void;
}

export interface DiscordBot {
  /// Connect a previously-created discord.js Client and start
  /// listening. Implementation deferred to the host — this scaffold
  /// keeps the wire-up surface a single function so the test suite
  /// can stub it.
  start(): Promise<void>;
  /// Dispatch a slash command invocation as if it had come from the
  /// real gateway. Returns the parsed user-input string for assertion.
  handleSlashCommand(
    name: DiscordCommandName,
    args: Record<string, string | number | boolean>,
  ): Promise<string>;
  /// Dispatch a raw chat message. Returns the parsed user-input
  /// string or null if the message didn't mention the bot.
  handleMessage(raw: string): Promise<string | null>;
  /// Stop listening (idempotent).
  stop(): void;
  /// The slash-command specs that ought to be registered with the
  /// gateway. Host wires these to ApplicationCommandBuilder.
  commandSpecs(): typeof DISCORD_COMMAND_SPECS;
}

export interface DiscordBotConfig {
  /// User ID the bot is logged in as — used to detect @mentions.
  botUserId: string;
  /// DecisionLoop the bot routes free-text commands into.
  loop: DecisionLoop;
  /// Optional side-effect sink for voice connect/disconnect + ack.
  sink?: DiscordEventSink;
}

/// Construct an in-process bot. Hosts compose this with an actual
/// discord.js Client by calling `handleSlashCommand` /
/// `handleMessage` from gateway event handlers.
export function createDiscordBot(config: DiscordBotConfig): DiscordBot {
  let started = false;
  return {
    async start() {
      started = true;
    },
    stop() {
      started = false;
    },
    commandSpecs() {
      return DISCORD_COMMAND_SPECS;
    },
    async handleSlashCommand(name, args) {
      if (!started) throw new Error("bot not started");
      const parsed = buildUserInputFromSlashCommand(name, args);
      if (parsed.shouldJoin) await config.sink?.onJoinRequested?.();
      if (parsed.shouldLeave) await config.sink?.onLeaveRequested?.();
      config.loop.enqueueUserInput(parsed.userInput);
      await config.sink?.onAck?.(parsed.userInput);
      return parsed.userInput;
    },
    async handleMessage(raw) {
      if (!started) throw new Error("bot not started");
      const parsed = parseMentionMessage(raw, config.botUserId);
      if (!parsed) return null;
      config.loop.enqueueUserInput(parsed.userInput);
      await config.sink?.onAck?.(parsed.userInput);
      return parsed.userInput;
    },
  };
}
