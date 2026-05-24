//! End-to-end test for the IPC server: spawn it, connect via UDS,
//! send Hello + SetTransport + CreateSequencerSource + RequestState,
//! verify ack/state flow.

use omm_engine::ipc::codec::{read_frame, write_frame};
use omm_engine::ipc::{spawn_audio_task, AudioTaskConfig, IpcServerConfig};
use omm_protocol::{EngineCommand, EngineEvent, TimeSignature, Transport, VoiceType};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

async fn boot_server() -> (std::path::PathBuf, tokio::task::JoinHandle<()>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("test.sock");
    // Keep `dir` alive by leaking it — the test is short, the OS will
    // reclaim. Tempdir auto-cleanup would race with the server task.
    std::mem::forget(dir);

    let (audio_tx, _audio_handle) = spawn_audio_task(AudioTaskConfig::default());
    let cfg = IpcServerConfig {
        socket_path: socket_path.clone(),
        audio_tx,
    };
    let socket_for_server = socket_path.clone();
    let handle = tokio::spawn(async move {
        let _ = omm_engine::ipc::serve(IpcServerConfig {
            socket_path: socket_for_server,
            audio_tx: cfg.audio_tx,
        })
        .await;
    });
    // Wait for the listener to come up.
    for _ in 0..100 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(socket_path.exists(), "server socket never appeared");
    (socket_path, handle)
}

#[tokio::test]
async fn hello_returns_helloack() {
    let (path, _server) = boot_server().await;
    let stream = UnixStream::connect(&path).await.expect("connect");
    let (read, write) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(read);
    let mut writer = tokio::io::BufWriter::new(write);

    write_frame(
        &mut writer,
        &EngineCommand::Hello {
            client_name: "test".to_string(),
            client_version: "0.0.1".to_string(),
        },
    )
    .await
    .unwrap();
    writer.flush().await.unwrap();

    let ev: EngineEvent = read_frame(&mut reader).await.unwrap();
    assert!(matches!(
        ev,
        EngineEvent::HelloAck {
            sample_rate: 48_000,
            ..
        }
    ));
}

#[tokio::test]
async fn set_transport_then_request_state_round_trips() {
    let (path, _server) = boot_server().await;
    let stream = UnixStream::connect(&path).await.expect("connect");
    let (read, write) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(read);
    let mut writer = tokio::io::BufWriter::new(write);

    // Handshake.
    write_frame(
        &mut writer,
        &EngineCommand::Hello {
            client_name: "test".to_string(),
            client_version: "0.0.1".to_string(),
        },
    )
    .await
    .unwrap();
    writer.flush().await.unwrap();
    let _ack: EngineEvent = read_frame(&mut reader).await.unwrap();

    // SetTransport.
    write_frame(
        &mut writer,
        &EngineCommand::SetTransport {
            transport: Transport::new(140.0, TimeSignature::FOUR_FOUR),
        },
    )
    .await
    .unwrap();
    writer.flush().await.unwrap();
    let ack: EngineEvent = read_frame(&mut reader).await.unwrap();
    assert!(matches!(ack, EngineEvent::Ack { .. }));

    // CreateSequencerSource.
    write_frame(
        &mut writer,
        &EngineCommand::CreateSequencerSource {
            source_instance_id: "seq:e2e".to_string(),
            voice_type: VoiceType::SineAdsr,
            polyphony: 8,
        },
    )
    .await
    .unwrap();
    writer.flush().await.unwrap();
    let ack: EngineEvent = read_frame(&mut reader).await.unwrap();
    assert!(matches!(ack, EngineEvent::Ack { .. }));

    // RequestState — wait briefly so engine frame > 0.
    tokio::time::sleep(Duration::from_millis(50)).await;
    write_frame(&mut writer, &EngineCommand::RequestState)
        .await
        .unwrap();
    writer.flush().await.unwrap();
    let snap: EngineEvent = read_frame(&mut reader).await.unwrap();
    match snap {
        EngineEvent::StateSnapshot {
            engine_frame,
            sample_rate,
        } => {
            assert_eq!(sample_rate, 48_000);
            assert!(engine_frame > 0, "engine should have advanced past 0");
        }
        other => panic!("expected StateSnapshot, got {other:?}"),
    }
}
