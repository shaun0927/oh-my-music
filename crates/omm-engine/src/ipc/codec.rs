//! Length-prefixed MessagePack framing over an `AsyncRead`/`AsyncWrite`
//! pair. Matches the protocol contract in `docs/ARCHITECTURE.md` §8.1:
//!
//!   `[u32 little-endian payload length][MessagePack payload]`

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum payload bytes we accept on a single message. Guard against
/// runaway allocations from a misbehaving peer.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("frame exceeds max ({0} > {MAX_FRAME_BYTES})")]
    FrameTooLarge(u32),
    #[error("decode: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
    #[error("encode: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    #[error("peer closed")]
    Closed,
}

/// Read one length-prefixed MessagePack frame and decode as `T`.
/// Returns `CodecError::Closed` if the peer closed at the frame
/// boundary (EOF before any length byte).
pub async fn read_frame<R, T>(reader: &mut R) -> Result<T, CodecError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0_u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(CodecError::Closed);
        }
        Err(err) => return Err(err.into()),
    }
    let len = u32::from_le_bytes(len_buf);
    if len as usize > MAX_FRAME_BYTES {
        return Err(CodecError::FrameTooLarge(len));
    }
    let mut buf = vec![0_u8; len as usize];
    reader.read_exact(&mut buf).await?;
    let value: T = rmp_serde::from_slice(&buf)?;
    Ok(value)
}

/// Encode `value` as MessagePack and write it as a length-prefixed
/// frame to `writer`. Caller is expected to `flush` between batches.
pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), CodecError>
where
    W: AsyncWrite + Unpin,
    T: Serialize + ?Sized,
{
    let payload = rmp_serde::to_vec_named(value)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(CodecError::FrameTooLarge(payload.len() as u32));
    }
    let len = (payload.len() as u32).to_le_bytes();
    writer.write_all(&len).await?;
    writer.write_all(&payload).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use omm_protocol::{EngineCommand, EngineEvent};
    use tokio::io::duplex;

    #[tokio::test]
    async fn round_trip_engine_command_via_duplex() {
        let (mut a, mut b) = duplex(64 * 1024);
        let cmd = EngineCommand::Hello {
            client_name: "test".to_string(),
            client_version: "0.1.0".to_string(),
        };
        write_frame(&mut a, &cmd).await.unwrap();
        a.flush().await.unwrap();
        let back: EngineCommand = read_frame(&mut b).await.unwrap();
        assert_eq!(back, cmd);
    }

    #[tokio::test]
    async fn round_trip_engine_event_via_duplex() {
        let (mut a, mut b) = duplex(64 * 1024);
        let ev = EngineEvent::HelloAck {
            engine_version: "0.1.0".to_string(),
            sample_rate: 48_000,
        };
        write_frame(&mut a, &ev).await.unwrap();
        a.flush().await.unwrap();
        let back: EngineEvent = read_frame(&mut b).await.unwrap();
        assert_eq!(back, ev);
    }

    #[tokio::test]
    async fn closed_peer_returns_closed_error() {
        let (a, mut b) = duplex(64 * 1024);
        drop(a);
        let result: Result<EngineCommand, _> = read_frame(&mut b).await;
        assert!(matches!(result, Err(CodecError::Closed)));
    }
}
