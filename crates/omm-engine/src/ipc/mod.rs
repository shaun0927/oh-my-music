//! IPC subsystem: Unix-domain-socket server + length-prefixed
//! MessagePack codec + dedicated audio-thread bridge.

pub mod audio_task;
pub mod codec;
pub mod server;

pub use audio_task::{spawn_audio_task, AudioTaskConfig, CommandEnvelope};
pub use codec::{read_frame, write_frame, CodecError, MAX_FRAME_BYTES};
pub use server::{default_socket_path, serve, IpcServerConfig};
