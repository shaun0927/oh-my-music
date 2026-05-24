// Root Ink component for the TUI. Combines the four scaffold panels
// (status, sources, decisions, input) into one screen. Rendering
// behaviour is intentionally minimal so the tests can focus on the
// pure formatters in `./format.ts`.

import React from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";

import type { LlmContext, RecentDecision } from "../../agent/context";
import {
  decisionsToLines,
  formatDb,
  formatSourceRow,
  renderMeterBar,
  statusLine,
  type SourceMeterRow,
} from "./format";

export interface AppProps {
  context: LlmContext;
  decisions: RecentDecision[];
  sourceRows: SourceMeterRow[];
  /// Called when the user submits a command (free-text or /<verb>).
  onSubmit: (text: string) => void;
  /// Called when the user hits Ctrl-C.
  onQuit?: () => void;
}

export function App(props: AppProps) {
  const [value, setValue] = React.useState("");
  useInput((_input, key) => {
    if (key.ctrl && (key.return || _input === "c")) {
      props.onQuit?.();
    }
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Box borderStyle="single" paddingX={1}>
        <Text>{statusLine(props.context)}</Text>
      </Box>

      <Box marginTop={1} flexDirection="column">
        <Text bold>Sources</Text>
        {props.sourceRows.length === 0 ? (
          <Text>  (none)</Text>
        ) : (
          props.sourceRows.map((row) => (
            <Text key={row.label}>{formatSourceRow(row)}</Text>
          ))
        )}
      </Box>

      <Box marginTop={1} flexDirection="column">
        <Text bold>Master</Text>
        <Text>
          {"  "}
          {renderMeterBar(props.context.currentState.masterPeakDb)}{"  "}
          {formatDb(props.context.currentState.masterPeakDb)} dB peak
          {"  "}
          GR {formatDb(-props.context.currentState.limiterGainReductionDb)} dB
        </Text>
      </Box>

      <Box marginTop={1} flexDirection="column">
        <Text bold>Decisions</Text>
        {decisionsToLines(props.decisions).map((line, i) => (
          <Text key={i}>{line}</Text>
        ))}
      </Box>

      <Box marginTop={1}>
        <Text>{"> "}</Text>
        <TextInput
          value={value}
          onChange={setValue}
          onSubmit={(text) => {
            if (text.trim().length > 0) props.onSubmit(text);
            setValue("");
          }}
        />
      </Box>
    </Box>
  );
}
