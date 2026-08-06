//! Worker protocol state-machine tests.
use std::io::Write as _;
use std::path::PathBuf;

use serde::Serialize;

use super::protocol::{
    BoundedBuffer, InitializeSession, PACKAGE_VERSION, RequestFrame, ResponseFrame, ValidatedInitialize,
    WorkerProtocolInfo, WorkerRequest, WorkerResponse, WorkerState, decode_frame, handle_worker_initialize,
    read_bounded_frame, run_worker, write_bounded_frame,
};
use super::wire::FetchWire;
use super::*;
use crate::{Error, FetchOptions, NetworkPolicy};

fn encoded(value: &impl Serialize) -> Vec<u8> {
    let mut out = Vec::new();
    write_bounded_frame(&mut out, value, MAX_WORKER_FRAME_BYTES).unwrap();
    out
}

#[test]
fn bounded_framing_rejects_truncation_and_overflow() {
    assert_eq!(
        read_bounded_frame(&mut std::io::Cursor::new(Vec::<u8>::new()), 8)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    let mut partial = 4_u32.to_be_bytes().to_vec();
    partial.extend_from_slice(&[1, 2, 3]);
    assert_eq!(
        read_bounded_frame(&mut std::io::Cursor::new(partial), 8)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    let mut buffer = BoundedBuffer::new(8);
    assert_eq!(buffer.write(b"12345678").unwrap(), 8);
    assert!(buffer.write(b"9").is_err());
}

#[test]
fn protocol_v1_golden_encoding_is_stable() {
    let info = WorkerProtocolInfo {
        magic: WORKER_PROTOCOL_MAGIC,
        protocol_version: 1,
        package_version: "1.2.3".into(),
    };
    assert_eq!(
        postcard::to_stdvec(&info).unwrap(),
        [WORKER_PROTOCOL_MAGIC.as_slice(), &[1, 5], b"1.2.3"].concat()
    );
    assert_eq!(
        postcard::to_stdvec(&RequestFrame {
            id: 7,
            request: WorkerRequest::Shutdown
        })
        .unwrap(),
        vec![7, 3]
    );
    assert_eq!(
        postcard::to_stdvec(&ResponseFrame {
            id: 7,
            response: WorkerResponse::ShutdownAck
        })
        .unwrap(),
        vec![7, 7]
    );
}

#[test]
fn schema_is_json_blob_inside_postcard() {
    use crate::schema::FieldKind;
    let schema = crate::schema::ExtractSchema::builder()
        .field("title", "h1", FieldKind::Text)
        .build()
        .unwrap();
    let frame = RequestFrame {
        id: 1,
        request: WorkerRequest::Fetch(FetchWire::from_options(
            &FetchOptions::new("https://example.com").schema(schema),
        )),
    };
    let decoded: RequestFrame = decode_frame(&postcard::to_stdvec(&frame).unwrap()).unwrap();
    let WorkerRequest::Fetch(fetch) = decoded.request else {
        panic!("fetch")
    };
    assert_eq!(fetch.into_options().unwrap().extract_schema.unwrap().fields().len(), 1);
}

#[test]
fn duplicate_initialization_preserves_state() {
    for initial in [
        WorkerState::Ready {
            user_agent: Some("SessionBot/1".into()),
        },
        WorkerState::Failed,
    ] {
        let mut state = initial.clone();
        let response = handle_worker_initialize(
            InitializeSession {
                permissive_network: false,
                user_agent: None,
                cookies: Vec::new(),
                cookie_scope: None,
                config_dir: PathBuf::from("/unused"),
                temporary_storage: true,
            },
            &mut state,
        );
        assert!(matches!(response, WorkerResponse::Error(_)));
        assert!(state == initial);
    }
}

#[test]
fn initialization_validates_before_state_mutation() {
    let mut state = WorkerState::AwaitingInitialize;
    let response = handle_worker_initialize(
        InitializeSession {
            permissive_network: false,
            user_agent: None,
            cookies: Vec::new(),
            cookie_scope: None,
            config_dir: PathBuf::from("relative"),
            temporary_storage: true,
        },
        &mut state,
    );
    assert!(matches!(response, WorkerResponse::Error(_)));
    assert!(state == WorkerState::AwaitingInitialize);
}

#[test]
fn initialization_sanitizes_ua_and_scope() {
    let dir = tempfile::tempdir().unwrap();
    let value = ValidatedInitialize::from_wire(InitializeSession {
        permissive_network: false,
        user_agent: Some("Bot\r\nInjected\0".into()),
        cookies: Vec::new(),
        cookie_scope: Some("https://user:secret@example.com/path".into()),
        config_dir: dir.path().to_path_buf(),
        temporary_storage: true,
    })
    .unwrap();
    assert_eq!(value.policy, NetworkPolicy::STRICT);
    assert_eq!(value.user_agent.as_deref(), Some("Bot  Injected "));
    assert_eq!(value.cookie_scope.as_deref(), Some("https://example.com/path"));
}

#[test]
fn worker_emits_info_and_shutdown_ack() {
    let mut output = Vec::new();
    run_worker(
        &mut std::io::Cursor::new(encoded(&RequestFrame {
            id: 9,
            request: WorkerRequest::Shutdown,
        })),
        &mut output,
    )
    .unwrap();
    let mut output = std::io::Cursor::new(output);
    let info: WorkerProtocolInfo =
        decode_frame(&read_bounded_frame(&mut output, MAX_WORKER_PROTOCOL_INFO_BYTES).unwrap()).unwrap();
    assert_eq!(info.magic, WORKER_PROTOCOL_MAGIC);
    assert_eq!(info.protocol_version, WORKER_PROTOCOL_VERSION);
    assert_eq!(info.package_version, PACKAGE_VERSION);
    let response: ResponseFrame =
        decode_frame(&read_bounded_frame(&mut output, MAX_WORKER_FRAME_BYTES).unwrap()).unwrap();
    assert_eq!(response.id, 9);
    assert!(matches!(response.response, WorkerResponse::ShutdownAck));
}

#[test]
fn malformed_frame_is_worker_error() {
    let mut truncated = 4_u32.to_be_bytes().to_vec();
    truncated.extend_from_slice(&[1, 2, 3]);
    assert!(matches!(
        run_worker(&mut std::io::Cursor::new(truncated), &mut Vec::new()),
        Err(Error::WorkerUnavailable { .. })
    ));
}
