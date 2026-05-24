// Real Unix-domain-socket client for the omm-engine IPC server.
// Length-prefixed MessagePack framing matches
// `crates/omm-engine/src/ipc/codec.rs`.

import { encode, decode } from "@msgpack/msgpack";
import { createConnection, type Socket } from "node:net";

import type {
  EngineNoteEvent,
} from "./music/theory";

const MAX_FRAME_BYTES = 4 * 1024 * 1024;

export interface IpcConnectOptions {
  /// Absolute Unix-domain-socket path.
  socketPath: string;
  /// Per-request response timeout in milliseconds. Defaults to 3000.
  requestTimeoutMs?: number;
}

/// Lowest-level framed connection: send any value as a
/// length-prefixed MessagePack frame, await the next inbound frame.
export class IpcConnection {
  private socket: Socket;
  private buffer: Buffer = Buffer.alloc(0);
  private inboxResolve: ((value: unknown) => void) | null = null;
  private inboxReject: ((err: Error) => void) | null = null;
  private closed = false;
  private requestTimeoutMs: number;

  private constructor(socket: Socket, timeoutMs: number) {
    this.socket = socket;
    this.requestTimeoutMs = timeoutMs;
    this.socket.on("data", (chunk: Buffer) => this.onData(chunk));
    this.socket.on("end", () => this.onClose());
    this.socket.on("error", (err) => this.onError(err));
  }

  static async connect(opts: IpcConnectOptions): Promise<IpcConnection> {
    const timeoutMs = opts.requestTimeoutMs ?? 3000;
    return new Promise((resolve, reject) => {
      const socket = createConnection({ path: opts.socketPath });
      socket.once("connect", () => resolve(new IpcConnection(socket, timeoutMs)));
      socket.once("error", reject);
    });
  }

  /// Send `value` as a length-prefixed MessagePack frame and resolve
  /// with the next inbound frame from the peer.
  async request<TIn, TOut>(value: TIn): Promise<TOut> {
    if (this.closed) throw new Error("connection closed");
    if (this.inboxResolve) throw new Error("request in flight (no pipelining)");

    const payload = encode(value as unknown as object);
    if (payload.length > MAX_FRAME_BYTES) {
      throw new Error(`frame too large: ${payload.length}`);
    }
    const header = Buffer.alloc(4);
    header.writeUInt32LE(payload.length, 0);
    this.socket.write(header);
    this.socket.write(Buffer.from(payload));

    return new Promise<TOut>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.inboxResolve = null;
        this.inboxReject = null;
        reject(new Error(`request timed out after ${this.requestTimeoutMs} ms`));
      }, this.requestTimeoutMs);
      this.inboxResolve = (val: unknown) => {
        clearTimeout(timer);
        resolve(val as TOut);
      };
      this.inboxReject = (err: Error) => {
        clearTimeout(timer);
        reject(err);
      };
    });
  }

  close(): void {
    this.closed = true;
    this.socket.end();
  }

  private onData(chunk: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    this.tryDeliverFrame();
  }

  private tryDeliverFrame(): void {
    if (this.buffer.length < 4) return;
    const len = this.buffer.readUInt32LE(0);
    if (len > MAX_FRAME_BYTES) {
      this.onError(new Error(`peer frame too large: ${len}`));
      return;
    }
    if (this.buffer.length < 4 + len) return;
    const payload = this.buffer.subarray(4, 4 + len);
    const rest = this.buffer.subarray(4 + len);
    this.buffer = Buffer.from(rest);
    const value = decode(payload);
    if (this.inboxResolve) {
      const r = this.inboxResolve;
      this.inboxResolve = null;
      this.inboxReject = null;
      r(value);
    }
  }

  private onClose(): void {
    this.closed = true;
    if (this.inboxReject) {
      const r = this.inboxReject;
      this.inboxResolve = null;
      this.inboxReject = null;
      r(new Error("peer closed"));
    }
  }

  private onError(err: Error): void {
    this.closed = true;
    if (this.inboxReject) {
      const r = this.inboxReject;
      this.inboxResolve = null;
      this.inboxReject = null;
      r(err);
    }
  }
}

// -- Typed EngineClient ---------------------------------------------------

export interface TransportSpec {
  bpm: number;
  time_signature: { numerator: number; denominator: number };
  swing?: number;
}

export type ScheduleTriggerSpec =
  | { Frame: number }
  | { MusicalTime: { bar: number; beat: number; tick: number } }
  | { RelativeMusical: { bars: number; beats: number } }
  | "NextBarBoundary"
  | "NextBeatBoundary";

export interface EngineClient {
  hello(name: string, version: string): Promise<unknown>;
  setTransport(transport: TransportSpec): Promise<unknown>;
  createSequencerSource(
    sourceInstanceId: string,
    voiceType: "SineAdsr" | "SawAdsr",
    polyphony: number,
  ): Promise<unknown>;
  removeSequencerSource(sourceInstanceId: string, fadeMs: number): Promise<unknown>;
  scheduleNotes(
    sourceInstanceId: string,
    notes: EngineNoteEvent[],
    trigger: ScheduleTriggerSpec,
    loopBars?: number,
  ): Promise<unknown>;
  clearNotes(sourceInstanceId: string, from?: { bar: number; beat: number; tick: number }): Promise<unknown>;
  requestState(): Promise<unknown>;
  // Legacy stub surface — kept so the existing tools/*.ts code keeps
  // compiling. Forwarded into `request` without a typed schema.
  sendParamBatch(batch: unknown): Promise<void>;
  loadGlicolCode(code: string, transitionMs: number): Promise<void>;
  close(): void;
}

export function createEngineClient(connection?: IpcConnection): EngineClient {
  if (!connection) {
    return makeStubClient();
  }
  return {
    hello: (name, version) =>
      connection.request({ Hello: { client_name: name, client_version: version } }),
    setTransport: (transport) =>
      connection.request({ SetTransport: { transport } }),
    createSequencerSource: (id, voiceType, polyphony) =>
      connection.request({
        CreateSequencerSource: {
          source_instance_id: id,
          voice_type: voiceType,
          polyphony,
        },
      }),
    removeSequencerSource: (id, fadeMs) =>
      connection.request({
        RemoveSequencerSource: { source_instance_id: id, fade_ms: fadeMs },
      }),
    scheduleNotes: (id, notes, trigger, loopBars) =>
      connection.request({
        ScheduleNotes: {
          batch: { source_instance_id: id, events: notes, loop_bars: loopBars ?? null },
          trigger,
        },
      }),
    clearNotes: (id, from) =>
      connection.request({ ClearNotes: { source_instance_id: id, from: from ?? null } }),
    requestState: () => connection.request("RequestState"),
    sendParamBatch: async (batch) => {
      // Existing tools/*.ts emit plain JSON objects; map them onto the
      // closest IPC variants. Unknown shapes fall back to console log.
      const tagged = batch as { type?: string };
      switch (tagged.type) {
        case "set_transport": {
          const t = batch as { type: string; transport: TransportSpec };
          await connection.request({ SetTransport: { transport: t.transport } });
          break;
        }
        case "create_sequencer_source": {
          const c = batch as {
            type: string;
            sourceInstanceId: string;
            voiceType: "SineAdsr" | "SawAdsr";
            polyphony: number;
          };
          await connection.request({
            CreateSequencerSource: {
              source_instance_id: c.sourceInstanceId,
              voice_type: c.voiceType,
              polyphony: c.polyphony,
            },
          });
          break;
        }
        case "remove_sequencer_source": {
          const r = batch as { type: string; sourceInstanceId: string; fadeMs: number };
          await connection.request({
            RemoveSequencerSource: {
              source_instance_id: r.sourceInstanceId,
              fade_ms: r.fadeMs,
            },
          });
          break;
        }
        case "schedule_notes": {
          const s = batch as {
            type: string;
            sourceInstanceId: string;
            notes: EngineNoteEvent[];
            trigger: ScheduleTriggerSpec;
            loopBars?: number;
          };
          await connection.request({
            ScheduleNotes: {
              batch: {
                source_instance_id: s.sourceInstanceId,
                events: s.notes,
                loop_bars: s.loopBars ?? null,
              },
              trigger: s.trigger,
            },
          });
          break;
        }
        case "clear_notes": {
          const c = batch as {
            type: string;
            sourceInstanceId: string;
            fromBar?: number;
            fromBeat?: number;
          };
          await connection.request({
            ClearNotes: {
              source_instance_id: c.sourceInstanceId,
              from:
                c.fromBar !== undefined
                  ? { bar: c.fromBar, beat: c.fromBeat ?? 0, tick: 0 }
                  : null,
            },
          });
          break;
        }
        default:
          console.log("sendParamBatch (unmapped)", batch);
      }
    },
    loadGlicolCode: async (code, transitionMs) => {
      await connection.request({ GlicolLoadCode: { code, transition_ms: transitionMs } });
    },
    close: () => connection.close(),
  };
}

function makeStubClient(): EngineClient {
  return {
    async hello() {
      return { stub: true };
    },
    async setTransport() {
      return { stub: true };
    },
    async createSequencerSource() {
      return { stub: true };
    },
    async removeSequencerSource() {
      return { stub: true };
    },
    async scheduleNotes() {
      return { stub: true };
    },
    async clearNotes() {
      return { stub: true };
    },
    async requestState() {
      return { stub: true };
    },
    async sendParamBatch(batch) {
      console.log("sendParamBatch (stub)", batch);
    },
    async loadGlicolCode(code, transitionMs) {
      console.log("loadGlicolCode (stub)", { code, transitionMs });
    },
    close() {},
  };
}
