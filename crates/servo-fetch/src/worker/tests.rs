//! Worker protocol state-machine tests.
use std::io::Write as _;
use std::path::PathBuf;

use serde::Serialize;

use super::protocol::{
    BoundedBuffer, InitializeSession, RequestFrame, ResponseFrame, ValidatedInitialize, WorkerProtocolInfo,
    WorkerRequest, WorkerResponse, WorkerState, decode_frame, handle_worker_initialize, read_bounded_frame, run_worker,
    write_bounded_frame,
};
use super::wire::{CrawlWire, FetchWire};
use super::*;
use crate::{CrawlOptions, Error, FetchOptions, NetworkPolicy};

fn encoded(value: &impl Serialize) -> Vec<u8> {
    let mut out = Vec::new();
    write_bounded_frame(&mut out, value, MAX_WORKER_FRAME_BYTES).unwrap();
    out
}

#[test]
fn bounded_framing_distinguishes_clean_eof_from_invalid_frames() {
    let clean = read_bounded_frame(&mut std::io::Cursor::new(Vec::<u8>::new()), 8).unwrap_err();
    assert_eq!(clean.kind(), std::io::ErrorKind::UnexpectedEof);

    for bytes in [vec![0], vec![0, 0], vec![0, 0, 0]] {
        let error = read_bounded_frame(&mut std::io::Cursor::new(bytes), 8).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    for length in [0_u32, 9] {
        let error = read_bounded_frame(&mut std::io::Cursor::new(length.to_be_bytes()), 8).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    let mut truncated = 4_u32.to_be_bytes().to_vec();
    truncated.extend_from_slice(&[1, 2, 3]);
    let error = read_bounded_frame(&mut std::io::Cursor::new(truncated), 8).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

    let payload = vec![0xa5; 8];
    let mut exact = 8_u32.to_be_bytes().to_vec();
    exact.extend_from_slice(&payload);
    assert_eq!(
        read_bounded_frame(&mut std::io::Cursor::new(exact), 8).unwrap(),
        payload
    );

    let mut buffer = BoundedBuffer::new(8);
    assert_eq!(buffer.write(b"12345678").unwrap(), 8);
    assert!(buffer.write(b"9").is_err());

    let mut truncated = 4_u32.to_be_bytes().to_vec();
    truncated.extend_from_slice(&[1, 2, 3]);
    assert!(matches!(
        run_worker(&mut std::io::Cursor::new(truncated), &mut Vec::new()),
        Err(Error::WorkerUnavailable { .. })
    ));

    let oversized = u32::try_from(MAX_WORKER_REQUEST_FRAME_BYTES + 1).unwrap().to_be_bytes();
    assert!(matches!(
        run_worker(&mut std::io::Cursor::new(oversized), &mut Vec::new()),
        Err(Error::WorkerUnavailable { .. })
    ));
}

#[test]
fn protocol_golden_encoding_is_stable() {
    let info = WorkerProtocolInfo {
        magic: WORKER_PROTOCOL_MAGIC,
        package_version: "1.2.3".into(),
    };
    assert_eq!(
        postcard::to_stdvec(&info).unwrap(),
        [WORKER_PROTOCOL_MAGIC.as_slice(), &[5], b"1.2.3"].concat()
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
fn pre_init_errors_preserve_id_and_worker_continues() {
    let requests = [
        WorkerRequest::Fetch(FetchWire::from_options(&FetchOptions::new("https://example.com"))),
        WorkerRequest::Crawl(CrawlWire::from_options(&CrawlOptions::new("https://example.com"))),
    ];

    for request in requests {
        let input = [
            encoded(&RequestFrame { id: 41, request }),
            encoded(&RequestFrame {
                id: 42,
                request: WorkerRequest::Shutdown,
            }),
        ]
        .concat();
        let mut output = Vec::new();
        run_worker(&mut std::io::Cursor::new(input), &mut output).unwrap();
        let mut output = std::io::Cursor::new(output);

        let _: WorkerProtocolInfo =
            decode_frame(&read_bounded_frame(&mut output, MAX_WORKER_PROTOCOL_INFO_BYTES).unwrap()).unwrap();
        let error: ResponseFrame =
            decode_frame(&read_bounded_frame(&mut output, MAX_WORKER_FRAME_BYTES).unwrap()).unwrap();
        assert_eq!(error.id, 41);
        assert!(matches!(error.response, WorkerResponse::Error(_)));

        let shutdown: ResponseFrame =
            decode_frame(&read_bounded_frame(&mut output, MAX_WORKER_FRAME_BYTES).unwrap()).unwrap();
        assert_eq!(shutdown.id, 42);
        assert!(matches!(shutdown.response, WorkerResponse::ShutdownAck));
    }
}
