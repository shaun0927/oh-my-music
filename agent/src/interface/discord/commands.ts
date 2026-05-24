// Discord slash-command shapes + the parser that maps incoming
// commands into DecisionLoop-friendly user-input strings. Keeps the
// network-side bot code (bot.ts) thin and testable.

export type DiscordCommandName =
  | "omm-start"
  | "omm-stop"
  | "omm-mood"
  | "omm-play"
  | "omm-duck"
  | "omm-energy";

export interface SlashCommandSpec {
  name: DiscordCommandName;
  description: string;
  /// Param schema, JSON-Schema-style. The actual discord.js builder
  /// translates this; we keep it small so tests don't depend on
  /// discord.js.
  options: Array<{
    name: string;
    description: string;
    type: "string" | "number" | "integer";
    required: boolean;
    choices?: string[];
  }>;
}

export const DISCORD_COMMAND_SPECS: SlashCommandSpec[] = [
  { name: "omm-start", description: "Join the voice channel and start the agent.", options: [] },
  { name: "omm-stop", description: "Leave the voice channel and stop the agent.", options: [] },
  {
    name: "omm-mood",
    description: "Set the agent's musical mood.",
    options: [
      {
        name: "mood",
        description: "calm / focus / energetic / dark / bright / dreamy / minimal",
        type: "string",
        required: true,
        choices: ["calm", "focus", "energetic", "dark", "bright", "dreamy", "minimal"],
      },
    ],
  },
  {
    name: "omm-play",
    description: "Load a music file by URL or local path.",
    options: [
      { name: "url", description: "audio file URL or path", type: "string", required: true },
    ],
  },
  {
    name: "omm-duck",
    description: "Set sidechain duck amount in dB.",
    options: [
      { name: "amount_db", description: "0..24", type: "number", required: true },
    ],
  },
  {
    name: "omm-energy",
    description: "Set the target musical energy (0..1).",
    options: [
      { name: "target", description: "0..1", type: "number", required: true },
    ],
  },
];

/// Result of parsing one incoming Discord interaction or chat
/// message. The bot enqueues `userInput` into the DecisionLoop and,
/// for `/omm-start` / `/omm-stop`, applies the side effect
/// (`shouldJoin` / `shouldLeave`) directly.
export interface ParsedDiscordInput {
  userInput: string;
  shouldJoin: boolean;
  shouldLeave: boolean;
}

/// Build the LlmContext-bound user-input string from a parsed
/// slash-command invocation.  Pure function — easy to test.
export function buildUserInputFromSlashCommand(
  name: DiscordCommandName,
  args: Record<string, string | number | boolean>,
): ParsedDiscordInput {
  switch (name) {
    case "omm-start":
      return { userInput: "join voice channel and begin", shouldJoin: true, shouldLeave: false };
    case "omm-stop":
      return { userInput: "stop playback and leave", shouldJoin: false, shouldLeave: true };
    case "omm-mood":
      return { userInput: `set mood to ${args.mood}`, shouldJoin: false, shouldLeave: false };
    case "omm-play":
      return { userInput: `play track from ${args.url}`, shouldJoin: false, shouldLeave: false };
    case "omm-duck":
      return {
        userInput: `set sidechain duck amount to ${args.amount_db} dB`,
        shouldJoin: false,
        shouldLeave: false,
      };
    case "omm-energy":
      return {
        userInput: `set energy target to ${args.target}`,
        shouldJoin: false,
        shouldLeave: false,
      };
  }
}

/// Strip an @bot mention prefix from a chat message and return the
/// remaining free-text command, or null if the message doesn't
/// target the bot.
export function parseMentionMessage(
  raw: string,
  botUserId: string,
): { userInput: string } | null {
  const mention = `<@${botUserId}>`;
  const altMention = `<@!${botUserId}>`;
  if (!raw.includes(mention) && !raw.includes(altMention)) return null;
  let text = raw.replace(mention, "").replace(altMention, "").trim();
  if (!text) return null;
  return { userInput: text };
}
