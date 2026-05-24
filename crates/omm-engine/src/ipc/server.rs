//! Unix-Domain-Socket IPC server. Accepts connections, runs the
//! length-prefixed MessagePack codec from [`super::codec`], and
//! forwards `EngineCommand`s to the audio task spawned via
//! [`super::audio_task::spawn_audio_task`].

use crate::ipc::audio_task::CommandEnvelope;
use crate::ipc::codec::{read_frame, write_frame, CodecError};
use omm_protocol::{EngineCommand, EngineEvent};
use std::path::PathBuf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

pub struct IpcServerConfig {
    pub socket_path: PathBuf,
    pub audio_tx: mpsc::Sender<CommandEnvelope>,
}

/// Default socket path per `docs/ARCHITECTURE.md` §8.1.
pub fn default_socket_path() -> PathBuf {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        let mut p = PathBuf::from(runtime_dir);
        p.push("oh-my-music");
        p.push("engine.sock");
        p
    } else {
        // Per-uid fallback for macOS / non-XDG hosts.
        let uid = unsafe { libc_getuid() };
        PathBuf::from(format!("/tmp/oh-my-music-{uid}/engine.sock"))
    }
}

/// Try to read the calling process's uid without pulling in the libc
/// crate. We just need a stable suffix for the socket path; safety is
/// fine because `getuid` is async-signal-safe and reentrant.
unsafe fn libc_getuid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    getuid()
}

/// Bind, accept, and serve until shutdown. The listener is dropped on
/// return so the socket file vanishes from the filesystem.
pub async fn serve(config: IpcServerConfig) -> anyhow::Result<()> {
    if let Some(parent) = config.socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Best-effort remove a stale socket file before binding.
    let _ = std::fs::remove_file(&config.socket_path);
    let listener = UnixListener::bind(&config.socket_path)?;
    eprintln!("omm-engine listening on {}", config.socket_path.display());

    loop {
        let (stream, _addr) = listener.accept().await?;
        let audio_tx = config.audio_tx.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, audio_tx).await {
                eprintln!("connection error: {err}");
            }
        });
    }
}

async fn handle_connection(
    stream: UnixStream,
    audio_tx: mpsc::Sender<CommandEnvelope>,
) -> Result<(), CodecError> {
    let (read, write) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(read);
    let mut writer = tokio::io::BufWriter::new(write);
    let mut next_msg_id: u64 = 1;

    loop {
        let command: EngineCommand = match read_frame(&mut reader).await {
            Ok(c) => c,
            Err(CodecError::Closed) => return Ok(()),
            Err(err) => return Err(err),
        };
        let msg_id = next_msg_id;
        next_msg_id = next_msg_id.wrapping_add(1);

        let (resp_tx, resp_rx) = oneshot::channel();
        let envelope = CommandEnvelope {
            command,
            respond_to: resp_tx,
            msg_id,
        };
        if audio_tx.send(envelope).await.is_err() {
            // Audio task gone — server is shutting down.
            return Ok(());
        }
        let event: EngineEvent = match resp_rx.await {
            Ok(ev) => ev,
            Err(_) => continue,
        };
        write_frame(&mut writer, &event).await?;
        tokio::io::AsyncWriteExt::flush(&mut writer).await?;
    }
}

/// Test-only helper: connect, send a single Hello, await HelloAck.
#[cfg(test)]
pub async fn hello_round_trip(path: &std::path::Path) -> anyhow::Result<EngineEvent> {
    use tokio::io::{AsyncWriteExt, BufReader, BufWriter};
    let stream = UnixStream::connect(path).await?;
    let (read, write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut writer = BufWriter::new(write);
    let cmd = EngineCommand::Hello {
        client_name: "round-trip".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    write_frame(&mut writer, &cmd).await?;
    writer.flush().await?;
    Ok(read_frame(&mut reader).await?)
}
