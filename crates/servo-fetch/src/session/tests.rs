//! Session protocol, lifecycle, isolation, and resource-ordering tests.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Read as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::Semaphore;

#[cfg(unix)]
use super::supervisor::create_parent_lifeline;
use super::supervisor::{SupervisorCommand, SupervisorOwner, frame_wait_duration, receive_response, response_channel};
use super::*;
use crate::worker::protocol::{
    CrawlProgressState, PACKAGE_VERSION, RequestFrame, ResponseFrame, WorkerProtocolInfo, WorkerRequest,
    WorkerResponse, decode_frame, write_bounded_frame,
};
use crate::worker::wire::{
    CrawlProgressWire, CrawlResultWire, CrawlWire, MAX_OPERATION_WATCHDOG, MAX_SCREENSHOT_BYTES, PageWire,
    WorkerErrorWire, crawl_absolute_watchdog, crawl_watchdog, fetch_watchdog,
};
use crate::worker::{MAX_WORKER_BLOB_CHUNK_BYTES, MAX_WORKER_FRAME_BYTES, WORKER_PROTOCOL_MAGIC};
use crate::{CrawlPage, CrawlResult, Page};

fn encoded_frame(value: &impl Serialize) -> Vec<u8> {
    let mut frame = Vec::new();
    write_bounded_frame(&mut frame, value, MAX_WORKER_FRAME_BYTES).unwrap();
    frame
}

fn shell_frame(value: &impl Serialize) -> String {
    let encoded = encoded_frame(value);
    let mut escaped = String::with_capacity(encoded.len().saturating_mul(4));
    for byte in encoded {
        write!(escaped, "\\{byte:03o}").unwrap();
    }
    format!("printf '{escaped}'; ")
}

fn protocol_info_shell() -> String {
    shell_frame(&WorkerProtocolInfo {
        magic: WORKER_PROTOCOL_MAGIC,
        package_version: PACKAGE_VERSION.into(),
    })
}

fn response_shell(id: u64, response: WorkerResponse) -> String {
    shell_frame(&ResponseFrame { id, response })
}

fn initialized_shell(id: u64) -> String {
    response_shell(id, WorkerResponse::SessionInitialized)
}

fn shutdown_ack_shell(id: u64) -> String {
    response_shell(id, WorkerResponse::ShutdownAck)
}

fn page_with_declared_screenshot(size: Option<u32>) -> PageWire {
    let (mut page, screenshot) = PageWire::from_page(Page::default()).unwrap();
    assert!(screenshot.is_none());
    page.set_screenshot_png_bytes(size);
    page
}

fn empty_page_shell(id: u64) -> String {
    let (page, screenshot) = PageWire::from_page(Page::default()).unwrap();
    assert!(screenshot.is_none());
    format!(
        "{}{}",
        response_shell(id, WorkerResponse::FetchResult(page)),
        response_shell(id, WorkerResponse::FetchCompleted)
    )
}

#[cfg(unix)]
const READ_FRAME_SHELL: &str = concat!(
    "read_frame() { ",
    "out=$1; ",
    "set -- $(dd bs=1 count=4 2>/dev/null | od -An -tu1); ",
    "[ $# -eq 4 ] || exit 0; ",
    "length=$(( $1 * 16777216 + $2 * 65536 + $3 * 256 + $4 )); ",
    "if [ -n \"$out\" ]; then dd bs=1 count=$length of=\"$out\" 2>/dev/null; ",
    "else dd bs=1 count=$length of=/dev/null 2>/dev/null; fi; ",
    "}; ",
);

#[cfg(unix)]
fn scripted_worker_prefix() -> String {
    format!("{READ_FRAME_SHELL}{}", protocol_info_shell())
}

#[cfg(unix)]
fn session_script() -> String {
    format!(
        "{}read_frame; {}read_frame; {}",
        scripted_worker_prefix(),
        initialized_shell(1),
        shutdown_ack_shell(2)
    )
}

#[test]
fn page_wire_detaches_binary_payloads() {
    let size = MAX_WORKER_BLOB_CHUNK_BYTES + 1;
    let page = Page {
        screenshot_png: Some(vec![0xff; size]),
        extracted: Some(serde_json::json!({"ok": true, "count": 2})),
        ..Page::default()
    };
    let (wire, screenshot) = PageWire::from_page(page).unwrap();
    let encoded = postcard::to_stdvec(&wire).unwrap();
    assert!(encoded.len() < 1024);
    let wire: PageWire = decode_frame(&encoded).unwrap();
    let screenshot = screenshot.unwrap();
    assert_eq!(screenshot.len(), size);
    let decoded = wire.into_page(Some(screenshot)).unwrap();
    assert_eq!(decoded.extracted, Some(serde_json::json!({"ok": true, "count": 2})));
    let decoded = decoded.screenshot_png.unwrap();
    assert_eq!(decoded.len(), size);
    assert!(decoded.iter().all(|byte| *byte == 0xff));
}

#[test]
fn response_decoding_rejects_trailing_bytes_and_invalid_payloads() {
    let response = ResponseFrame {
        id: 7,
        response: WorkerResponse::ShutdownAck,
    };
    let mut encoded = postcard::to_stdvec(&response).unwrap();
    encoded.push(0);
    assert!(decode_frame::<ResponseFrame>(&encoded).is_err());
    assert!(decode_frame::<ResponseFrame>(&[u8::MAX]).is_err());
}

#[test]
fn crawl_stream_progress_counts_results_and_suppressions() {
    let mut progress = CrawlProgressState::default();
    let result = CrawlResult {
        url: "https://example.com/a".into(),
        depth: 0,
        fetched_at: UNIX_EPOCH,
        outcome: Ok(CrawlPage {
            title: None,
            content: "a".into(),
            links_found: 0,
        }),
    };
    assert!(matches!(
        progress.observe(crate::crawl::CrawlSessionEvent::Result(result)),
        WorkerResponse::CrawlResult(_)
    ));

    let event = progress.observe(crate::crawl::CrawlSessionEvent::Suppressed(
        crate::crawl::SuppressedPage {
            url: "https://example.com/b".into(),
            depth: 1,
            reason: crate::crawl::SuppressionReason::DuplicateContent,
        },
    ));
    let WorkerResponse::CrawlProgress(snapshot) = event else {
        panic!("suppression must emit a progress snapshot");
    };
    assert_eq!(
        snapshot,
        CrawlProgressWire {
            processed: 2,
            emitted: 1,
            suppressed: 1,
        }
    );
    let encoded = postcard::to_stdvec(&ResponseFrame {
        id: 7,
        response: WorkerResponse::CrawlProgress(snapshot),
    })
    .unwrap();
    let decoded: ResponseFrame = decode_frame(&encoded).unwrap();
    assert!(matches!(decoded.response, WorkerResponse::CrawlProgress(_)));
}

#[test]
fn crawl_error_round_trip_preserves_timeout_kind() {
    let result = CrawlResult {
        url: "https://example.com/slow".into(),
        depth: 1,
        fetched_at: UNIX_EPOCH,
        outcome: Err(Error::Timeout {
            url: "https://example.com/slow".into(),
            timeout: Duration::from_secs(7),
        }),
    };
    let decoded = CrawlResultWire::from_result(result).into_result().unwrap();
    assert!(matches!(
        decoded.outcome,
        Err(Error::Timeout { ref url, timeout })
            if url == "https://example.com/slow" && timeout == Duration::from_secs(7)
    ));
}

#[cfg(unix)]
#[test]
fn parent_lifeline_reports_eof_when_owner_drops() {
    let (read_end, write_end) = create_parent_lifeline().unwrap();
    for descriptor in [&read_end, &write_end] {
        let raw_fd = std::os::fd::AsRawFd::as_raw_fd(descriptor);
        assert!(raw_fd >= 3);
        // SAFETY: fcntl only reads flags from a descriptor owned by this test.
        #[allow(unsafe_code)]
        let flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFD) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }
    let mut read_end = std::fs::File::from(read_end);
    drop(write_end);
    let mut byte = [0];
    assert_eq!(read_end.read(&mut byte).unwrap(), 0);
}

#[test]
fn tempdir_is_deleted_before_permit_release() {
    let permits = Arc::new(Semaphore::new(1));
    let permit = permits.clone().try_acquire_owned().unwrap();
    let mut owner = SupervisorOwner::new(permit).unwrap();
    let config_dir = owner.config_dir();
    assert!(config_dir.is_dir());
    let (observed, receive) = std::sync::mpsc::channel();
    let contender = std::thread::spawn({
        move || {
            let _permit = crate::runtime::block_on(permits.acquire_owned()).unwrap().unwrap();
            observed.send(config_dir.exists()).unwrap();
        }
    });
    owner.release();
    assert!(!receive.recv_timeout(Duration::from_secs(1)).unwrap());
    contender.join().unwrap();
}

#[test]
fn broker_rejects_invalid_configuration() {
    let executable = WorkerCommand::new(std::env::current_exe().unwrap());
    let cases = [
        SessionBrokerConfig::default().worker_command(WorkerCommand::new("relative-worker")),
        SessionBrokerConfig::default(),
        SessionBrokerConfig::default()
            .max_sessions(0)
            .worker_command(executable.clone()),
        SessionBrokerConfig::default()
            .max_sessions(usize::MAX)
            .worker_command(executable.clone()),
        SessionBrokerConfig::default()
            .queue_capacity(usize::MAX)
            .worker_command(executable.clone()),
        SessionBrokerConfig::default()
            .max_sessions(1)
            .prewarm(2)
            .worker_command(executable.clone()),
        SessionBrokerConfig::default()
            .acquire_timeout(Duration::ZERO)
            .worker_command(executable),
    ];
    for config in cases {
        assert!(matches!(
            SessionBroker::new(config),
            Err(Error::InvalidSessionConfig { .. })
        ));
    }
}

#[test]
fn session_rejects_per_request_state_bypasses() {
    let options = FetchOptions::new("https://example.com/protected.pdf");
    assert!(matches!(
        validate_session_fetch(&options),
        Err(Error::UnsupportedSessionOperation {
            operation: "PDF extraction",
            ..
        })
    ));

    let mut fetch = FetchOptions::new("https://example.com");
    fetch
        .headers
        .insert(http::header::USER_AGENT, http::HeaderValue::from_static("other"));
    assert!(validate_session_fetch(&fetch).is_err());

    let mut crawl = CrawlOptions::new("https://example.com");
    crawl
        .headers
        .insert(http::header::COOKIE, http::HeaderValue::from_static("sid=other"));
    assert!(validate_session_crawl(&crawl).is_err());
}

#[test]
fn session_rejects_untransportable_operation_values_before_dispatch() {
    let fetch = FetchOptions::new("https://example.com").timeout(MAX_WIRE_DURATION + Duration::from_millis(1));
    assert!(validate_session_fetch(&fetch).is_err());

    let mut reserved_header = FetchOptions::new("https://example.com");
    reserved_header
        .headers
        .insert(http::header::HOST, http::HeaderValue::from_static("other.example"));
    assert!(matches!(
        validate_session_fetch(&reserved_header),
        Err(Error::InvalidHeader(_))
    ));

    let crawl = CrawlOptions::new("https://example.com").concurrency(MAX_WIRE_CRAWL_CONCURRENCY + 1);
    assert!(validate_session_crawl(&crawl).is_err());
    let crawl = CrawlOptions::new("https://example.com").delay(Some(MAX_WIRE_DURATION + Duration::from_millis(1)));
    assert!(validate_session_crawl(&crawl).is_err());
}

#[test]
fn absolute_deadline_bounds_each_idle_wait() {
    assert_eq!(
        frame_wait_duration(Duration::from_secs(30), Duration::from_millis(5)),
        Some(Duration::from_millis(5))
    );
    assert_eq!(
        frame_wait_duration(Duration::from_millis(5), Duration::from_secs(30)),
        Some(Duration::from_millis(5))
    );
    assert_eq!(frame_wait_duration(Duration::from_secs(30), Duration::ZERO), None);
}

#[test]
fn worker_watchdogs_include_all_valid_wait_phases() {
    let fetch = FetchOptions::new("https://example.com")
        .timeout(Duration::from_secs(5))
        .settle(Duration::from_secs(21));
    assert_eq!(fetch_watchdog(&fetch), Duration::from_secs(41));
    let crawl = CrawlOptions::new("https://example.com")
        .timeout(Duration::from_secs(5))
        .settle(Duration::from_secs(21))
        .delay(Some(Duration::from_secs(3)))
        .concurrency(4);
    assert_eq!(crawl_watchdog(&crawl), Duration::from_secs(58));
    assert!(crawl_absolute_watchdog(&crawl) > crawl_watchdog(&crawl));

    let extreme_fetch = FetchOptions::new("https://example.com")
        .timeout(Duration::MAX)
        .settle(Duration::MAX);
    assert_eq!(fetch_watchdog(&extreme_fetch), MAX_OPERATION_WATCHDOG);
    let extreme_crawl = CrawlOptions::new("https://example.com")
        .timeout(Duration::MAX)
        .settle(Duration::MAX)
        .delay(Some(Duration::MAX));
    assert_eq!(crawl_watchdog(&extreme_crawl), MAX_OPERATION_WATCHDOG);
    assert_eq!(crawl_absolute_watchdog(&extreme_crawl), MAX_OPERATION_WATCHDOG);
}

#[cfg(unix)]
fn scripted_broker(script: &str, args: impl IntoIterator<Item = OsString>, queue_capacity: usize) -> SessionBroker {
    SessionBroker::new(SessionBrokerConfig {
        max_sessions: 1,
        queue_capacity,
        acquire_timeout: Duration::from_secs(3),
        prewarm: 0,
        worker_command: WorkerCommand::new("/bin/sh").args(
            [OsString::from("-c"), OsString::from(script), OsString::from("worker")]
                .into_iter()
                .chain(args),
        ),
    })
    .unwrap()
}

#[cfg(unix)]
fn assert_fetch_protocol_failure(responses: &str) {
    let script = format!(
        "{}read_frame; {}read_frame; {responses}exec sleep 10",
        scripted_worker_prefix(),
        initialized_shell(1),
    );
    let broker = scripted_broker(&script, [], 0);
    let mut session = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
    let config_dir = session.supervisor.as_ref().unwrap().config_dir.clone();
    assert!(matches!(
        session.fetch_blocking(&FetchOptions::new("https://example.com")),
        Err(Error::WorkerUnavailable { .. })
    ));
    assert!(!config_dir.exists());
    assert!(
        session
            .fetch_blocking(&FetchOptions::new("https://example.com"))
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn protocol_info_validation_fails_closed() {
    let invalid_infos = [
        WorkerProtocolInfo {
            magic: *b"NOTWORK\0",
            package_version: PACKAGE_VERSION.into(),
        },
        WorkerProtocolInfo {
            magic: WORKER_PROTOCOL_MAGIC,
            package_version: format!("{PACKAGE_VERSION}-mismatch"),
        },
    ];
    for info in invalid_infos {
        let script = format!("{READ_FRAME_SHELL}{}exec sleep 10", shell_frame(&info));
        let broker = scripted_broker(&script, [], 0);
        assert!(matches!(
            broker.session_blocking(BrowserSessionConfig::new()),
            Err(Error::WorkerUnavailable { .. })
        ));
    }

    for script in [
        "printf '\\000\\000\\020\\001'; exec sleep 10",
        "printf '\\000\\000\\000\\001\\377'; exec sleep 10",
    ] {
        let broker = scripted_broker(script, [], 0);
        assert!(matches!(
            broker.session_blocking(BrowserSessionConfig::new()),
            Err(Error::WorkerUnavailable { .. })
        ));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn force_close_terminates_worker_process_group() {
    let directory = tempfile::tempdir().unwrap();
    let child_pid = directory.path().join("child-pid");
    let script = format!(
        "{}read_frame; {}sleep 60 & echo $! > \"$1\"; exec sleep 60",
        scripted_worker_prefix(),
        initialized_shell(1)
    );
    let broker = scripted_broker(&script, [child_pid.clone().into_os_string()], 0);
    let session = broker.session(BrowserSessionConfig::new()).await.unwrap();
    let config_dir = session.supervisor.as_ref().unwrap().config_dir.clone();
    let deadline = Instant::now() + Duration::from_secs(1);
    while !child_pid.exists() {
        assert!(Instant::now() < deadline, "worker descendant did not start");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let pid = std::fs::read_to_string(&child_pid)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();

    tokio::time::timeout(Duration::from_secs(3), session.force_close())
        .await
        .expect("process-group cleanup must be bounded")
        .unwrap();
    assert!(!config_dir.exists());
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        // SAFETY: signal 0 only probes the PID captured from the test child.
        #[allow(unsafe_code)]
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive {
            break;
        }
        assert!(Instant::now() < deadline, "worker descendant survived force close");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn drop_is_nonblocking_and_force_closes() {
    let script = format!(
        "{}read_frame; {}exec sleep 10",
        scripted_worker_prefix(),
        initialized_shell(1)
    );
    let broker = scripted_broker(&script, [], 0);
    let session = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
    let config_dir = session.supervisor.as_ref().unwrap().config_dir.clone();
    let started = Instant::now();
    drop(session);
    assert!(started.elapsed() < Duration::from_millis(100));

    let deadline = Instant::now() + Duration::from_secs(3);
    while config_dir.exists() {
        assert!(Instant::now() < deadline, "background force close did not complete");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn graceful_close_sends_shutdown_reaps_descendants_and_deletes_storage() {
    let directory = tempfile::tempdir().unwrap();
    let initialize_request = directory.path().join("initialize-request");
    let shutdown_request = directory.path().join("shutdown-request");
    let child_pid = directory.path().join("child-pid");
    let script = format!(
        "{}read_frame \"$1\"; {}sleep 60 & echo $! > \"$3\"; read_frame \"$2\"; {}exit 0",
        scripted_worker_prefix(),
        initialized_shell(1),
        shutdown_ack_shell(2)
    );
    let broker = scripted_broker(
        &script,
        [
            initialize_request.clone().into_os_string(),
            shutdown_request.clone().into_os_string(),
            child_pid.clone().into_os_string(),
        ],
        0,
    );
    let session = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
    let config_dir = session.supervisor.as_ref().unwrap().config_dir.clone();
    let deadline = Instant::now() + Duration::from_secs(1);
    while !child_pid.exists() {
        assert!(Instant::now() < deadline, "worker descendant did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    let pid = std::fs::read_to_string(&child_pid)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();

    session.close_blocking().unwrap();
    assert!(!config_dir.exists());

    let initialize: RequestFrame = decode_frame(&std::fs::read(initialize_request).unwrap()).unwrap();
    let WorkerRequest::Initialize(initialization) = initialize.request else {
        panic!("expected initialize request");
    };
    assert_eq!(initialization.config_dir, config_dir);
    let shutdown: RequestFrame = decode_frame(&std::fs::read(shutdown_request).unwrap()).unwrap();
    assert!(matches!(shutdown.request, WorkerRequest::Shutdown));

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        // SAFETY: signal 0 only probes the PID captured from the test child.
        #[allow(unsafe_code)]
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "worker descendant survived graceful shutdown"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn shutdown_faults_are_bounded_and_cleanup_storage() {
    let cases = [
        format!("{}exec sleep 10", shutdown_ack_shell(2)),
        format!("{}exit 7", shutdown_ack_shell(2)),
        format!("{}exec sleep 10", response_shell(2, WorkerResponse::CrawlCompleted)),
    ];
    for response in cases {
        let script = format!(
            "{}read_frame; {}read_frame; {response}",
            scripted_worker_prefix(),
            initialized_shell(1),
        );
        let broker = scripted_broker(&script, [], 0);
        let session = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
        let config_dir = session.supervisor.as_ref().unwrap().config_dir.clone();
        let started = Instant::now();
        assert!(session.close_blocking().is_err());
        assert!(started.elapsed() < Duration::from_secs(4));
        assert!(!config_dir.exists());
    }
}

#[cfg(unix)]
#[test]
fn malformed_fetch_sequences_are_terminal() {
    let oversized = u32::try_from(MAX_SCREENSHOT_BYTES + 1).unwrap();
    let sequences = [
        response_shell(99, WorkerResponse::FetchCompleted),
        response_shell(2, WorkerResponse::ScreenshotChunk(vec![1])),
        response_shell(2, WorkerResponse::FetchCompleted),
        format!(
            "{}{}",
            response_shell(2, WorkerResponse::FetchResult(page_with_declared_screenshot(None))),
            response_shell(2, WorkerResponse::FetchResult(page_with_declared_screenshot(None))),
        ),
        format!(
            "{}{}",
            response_shell(2, WorkerResponse::FetchResult(page_with_declared_screenshot(Some(1)))),
            response_shell(2, WorkerResponse::ScreenshotChunk(Vec::new())),
        ),
        format!(
            "{}{}",
            response_shell(2, WorkerResponse::FetchResult(page_with_declared_screenshot(None))),
            response_shell(2, WorkerResponse::ScreenshotChunk(vec![1])),
        ),
        format!(
            "{}{}",
            response_shell(2, WorkerResponse::FetchResult(page_with_declared_screenshot(Some(2)))),
            response_shell(2, WorkerResponse::ScreenshotChunk(vec![1, 2, 3])),
        ),
        format!(
            "{}{}{}",
            response_shell(2, WorkerResponse::FetchResult(page_with_declared_screenshot(Some(3)))),
            response_shell(2, WorkerResponse::ScreenshotChunk(vec![1, 2])),
            response_shell(2, WorkerResponse::FetchCompleted),
        ),
        format!(
            "{}{}",
            response_shell(2, WorkerResponse::FetchResult(page_with_declared_screenshot(None))),
            response_shell(
                2,
                WorkerResponse::Error(WorkerErrorWire::failure("engine", "late error"))
            ),
        ),
        response_shell(
            2,
            WorkerResponse::FetchResult(page_with_declared_screenshot(Some(oversized))),
        ),
    ];
    for sequence in sequences {
        assert_fetch_protocol_failure(&sequence);
    }
}

#[cfg(unix)]
#[test]
fn oversized_screenshot_chunk_is_terminal() {
    let directory = tempfile::tempdir().unwrap();
    let response_path = directory.path().join("oversized-chunk");
    let chunk_size = MAX_WORKER_BLOB_CHUNK_BYTES + 1;
    std::fs::write(
        &response_path,
        encoded_frame(&ResponseFrame {
            id: 2,
            response: WorkerResponse::ScreenshotChunk(vec![0; chunk_size]),
        }),
    )
    .unwrap();
    let script = format!(
        "{}read_frame; {}read_frame; {}cat \"$1\"; exec sleep 10",
        scripted_worker_prefix(),
        initialized_shell(1),
        response_shell(
            2,
            WorkerResponse::FetchResult(page_with_declared_screenshot(Some(u32::try_from(chunk_size).unwrap()))),
        ),
    );
    let broker = scripted_broker(&script, [response_path.into_os_string()], 0);
    let mut session = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
    assert!(matches!(
        session.fetch_blocking(&FetchOptions::new("https://example.com")),
        Err(Error::WorkerUnavailable { .. })
    ));
}

#[cfg(unix)]
#[test]
fn application_error_keeps_session_usable() {
    let script = format!(
        "{}read_frame; {}read_frame; {}read_frame; {}read_frame; {}",
        scripted_worker_prefix(),
        initialized_shell(1),
        response_shell(2, WorkerResponse::Error(WorkerErrorWire::failure("engine", "boom")),),
        empty_page_shell(3),
        shutdown_ack_shell(4)
    );
    let broker = scripted_broker(&script, [], 0);
    let mut session = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
    let config_dir = session.supervisor.as_ref().unwrap().config_dir.clone();

    assert!(
        session
            .fetch_blocking(&FetchOptions::new("https://example.com"))
            .is_err()
    );
    assert!(config_dir.exists(), "application error unexpectedly closed the session");
    session
        .fetch_blocking(&FetchOptions::new("https://example.com"))
        .unwrap();
    session.close_blocking().unwrap();
    assert!(!config_dir.exists());
}

#[cfg(unix)]
#[test]
fn screenshot_chunks_are_reassembled_by_supervisor() {
    let (page, screenshot) = PageWire::from_page(Page {
        screenshot_png: Some(vec![1, 2, 3, 4, 5]),
        ..Page::default()
    })
    .unwrap();
    assert_eq!(screenshot.unwrap(), vec![1, 2, 3, 4, 5]);
    let script = format!(
        "{}read_frame; {}read_frame; {}{}{}{}read_frame; {}",
        scripted_worker_prefix(),
        initialized_shell(1),
        response_shell(2, WorkerResponse::FetchResult(page)),
        response_shell(2, WorkerResponse::ScreenshotChunk(vec![1, 2])),
        response_shell(2, WorkerResponse::ScreenshotChunk(vec![3, 4, 5])),
        response_shell(2, WorkerResponse::FetchCompleted),
        shutdown_ack_shell(3)
    );
    let broker = scripted_broker(&script, [], 0);
    let mut session = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
    let page = session
        .fetch_blocking(&FetchOptions::screenshot("https://example.com", false))
        .unwrap();
    assert_eq!(page.screenshot_png, Some(vec![1, 2, 3, 4, 5]));
    session.close_blocking().unwrap();
}

#[cfg(unix)]
#[test]
fn force_cancel_releases_capacity_without_waiting_for_handle_drop() {
    let script = session_script();
    let broker = scripted_broker(&script, [], 1);
    let cancellation = SessionCancellation::new();
    let mut session = broker
        .session_blocking_with_cancellation(BrowserSessionConfig::new(), &cancellation)
        .unwrap();
    cancellation.cancel();
    let mut replacement = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
    replacement.cancel();
    session.cancel();
}

#[cfg(unix)]
#[test]
fn prewarmed_worker_is_clean_and_never_reused_after_close() {
    let directory = tempfile::tempdir().unwrap();
    let spawns = directory.path().join("spawns");
    let script = format!(
        "printf x >> \"$1\"; {}read_frame; {}read_frame; {}",
        scripted_worker_prefix(),
        initialized_shell(1),
        shutdown_ack_shell(2)
    );
    let broker = SessionBroker::new(SessionBrokerConfig {
        max_sessions: 1,
        queue_capacity: 0,
        acquire_timeout: Duration::from_secs(1),
        prewarm: 1,
        worker_command: WorkerCommand::new("/bin/sh").args([
            OsString::from("-c"),
            OsString::from(script),
            OsString::from("worker"),
            spawns.clone().into_os_string(),
        ]),
    })
    .unwrap();
    assert_eq!(std::fs::read(&spawns).unwrap(), b"x");
    broker
        .session_blocking(BrowserSessionConfig::new())
        .unwrap()
        .close_blocking()
        .unwrap();
    broker
        .session_blocking(BrowserSessionConfig::new())
        .unwrap()
        .close_blocking()
        .unwrap();
    assert_eq!(std::fs::read(spawns).unwrap(), b"xx");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bounded_queue_enforces_capacity_and_fifo_fairness() {
    let script = session_script();
    let saturated = scripted_broker(&script, [], 0);
    let held = saturated.session(BrowserSessionConfig::new()).await.unwrap();
    assert!(matches!(
        saturated.session(BrowserSessionConfig::new()).await,
        Err(Error::SessionBrokerFull)
    ));
    held.close().await.unwrap();

    let broker = Arc::new(scripted_broker(&script, [], 3));
    let held = broker.session(BrowserSessionConfig::new()).await.unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut tasks = Vec::new();
    for id in 0..3 {
        let broker = broker.clone();
        let order = order.clone();
        tasks.push(tokio::spawn(async move {
            let session = broker.session(BrowserSessionConfig::new()).await.unwrap();
            order.lock().unwrap().push(id);
            session.close().await.unwrap();
        }));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    held.close().await.unwrap();
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(*order.lock().unwrap(), vec![0, 1, 2]);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_cancellation_force_closes_before_permit_reuse() {
    let directory = tempfile::tempdir().unwrap();
    let gate = directory.path().join("gate");
    let starts = directory.path().join("starts");
    let script = format!(
        "printf x >> \"$1\"; while [ ! -e \"$2\" ]; do sleep 0.01; done; {}read_frame; {}read_frame; {}",
        scripted_worker_prefix(),
        initialized_shell(1),
        shutdown_ack_shell(2)
    );
    let broker = Arc::new(scripted_broker(
        &script,
        [starts.clone().into_os_string(), gate.clone().into_os_string()],
        1,
    ));
    let cancellation = SessionCancellation::new();
    let first_broker = broker.clone();
    let first_cancel = cancellation.clone();
    let first = tokio::spawn(async move {
        first_broker
            .session_with_cancellation(BrowserSessionConfig::new(), &first_cancel)
            .await
    });
    while std::fs::read(&starts).map_or(0, |bytes| bytes.len()) < 1 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cancellation.cancel();
    let second_broker = broker.clone();
    let second = tokio::spawn(async move { second_broker.session(BrowserSessionConfig::new()).await });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while std::fs::read(&starts).map_or(0, |bytes| bytes.len()) < 2 {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    std::fs::write(gate, b"open").unwrap();
    assert!(first.await.unwrap().is_err());
    second.await.unwrap().unwrap().close().await.unwrap();
}

#[cfg(unix)]
#[test]
fn stalled_crawl_consumer_does_not_block_force_close() {
    let event = || {
        WorkerResponse::CrawlResult(CrawlResultWire::from_result(CrawlResult {
            url: "https://example.com".into(),
            depth: 0,
            fetched_at: UNIX_EPOCH,
            outcome: Ok(CrawlPage {
                title: None,
                content: "x".into(),
                links_found: 0,
            }),
        }))
    };
    let script = format!(
        "{}read_frame; {}read_frame; {}{}exec sleep 10",
        scripted_worker_prefix(),
        initialized_shell(1),
        response_shell(2, event()),
        response_shell(2, event())
    );
    let broker = scripted_broker(&script, [], 1);
    let cancellation = SessionCancellation::new();
    let mut session = broker
        .session_blocking_with_cancellation(BrowserSessionConfig::new(), &cancellation)
        .unwrap();
    let (events, _receive_events) = crossbeam_channel::bounded(1);
    let (reply, receive) = response_channel();
    session
        .supervisor_mut()
        .unwrap()
        .send(SupervisorCommand::Crawl {
            request: CrawlWire::from_options(&CrawlOptions::new("https://example.com").delay(None)),
            events,
            reply,
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));

    cancellation.cancel();
    assert!(receive_response(&receive, "crawl cancellation").is_err());
    let mut replacement = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
    replacement.cancel();
    session.cancel();
}

#[cfg(unix)]
#[test]
fn crawl_progress_frames_extend_per_frame_watchdog() {
    let progress = |processed, suppressed| {
        response_shell(
            2,
            WorkerResponse::CrawlProgress(CrawlProgressWire {
                processed,
                emitted: 0,
                suppressed,
            }),
        )
    };
    let script = format!(
        "{}read_frame; {}read_frame; sleep 0.04; {}sleep 0.04; {}sleep 0.04; {}read_frame; {}",
        scripted_worker_prefix(),
        initialized_shell(1),
        progress(1, 1),
        progress(2, 2),
        response_shell(2, WorkerResponse::CrawlCompleted),
        shutdown_ack_shell(3)
    );
    let broker = scripted_broker(&script, [], 0);
    let mut session = broker.session_blocking(BrowserSessionConfig::new()).unwrap();
    let (events, _receive_events) = crossbeam_channel::bounded(1);
    let (reply, receive) = response_channel();
    let request = CrawlWire::from_options(
        &CrawlOptions::new("https://example.com")
            .timeout(Duration::from_millis(1))
            .delay(None),
    );
    session
        .supervisor_mut()
        .unwrap()
        .send(SupervisorCommand::Crawl { request, events, reply })
        .unwrap();
    let started = Instant::now();
    receive_response(&receive, "crawl").unwrap();
    assert!(started.elapsed() >= Duration::from_millis(100));
    session.close_blocking().unwrap();
}

#[tokio::test]
async fn ordinary_acquisition_failures_do_not_poison_cancellation() {
    let script = session_script();
    let broker = scripted_broker(&script, [], 0);
    let held = broker.session(BrowserSessionConfig::new()).await.unwrap();
    let cancellation = SessionCancellation::new();
    assert!(matches!(
        broker
            .session_with_cancellation(BrowserSessionConfig::new(), &cancellation)
            .await,
        Err(Error::SessionBrokerFull)
    ));
    assert!(
        !cancellation.is_cancelled(),
        "a failed acquisition must not cancel the caller's handle"
    );
    held.close().await.unwrap();
}
