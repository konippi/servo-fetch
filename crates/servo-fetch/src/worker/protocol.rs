//! Bounded typed protocol served by an isolated worker process.

use std::io::{BufWriter, Read, Write};
use std::mem::size_of;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::wire::{CrawlProgressWire, CrawlResultWire, CrawlWire, FetchWire, PageWire, WorkerErrorWire};
use super::{
    MAX_WORKER_BLOB_CHUNK_BYTES, MAX_WORKER_FRAME_BYTES, MAX_WORKER_PROTOCOL_INFO_BYTES,
    MAX_WORKER_REQUEST_FRAME_BYTES, WORKER_PROTOCOL_MAGIC, worker_error,
};
use crate::NetworkPolicy;
use crate::cookies::CookieWire;
use crate::error::{Error, Result};

pub(crate) const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
const FRAME_LENGTH_BYTES: usize = size_of::<u32>();

pub(crate) fn decode_frame<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let (value, remainder) = postcard::take_from_bytes(bytes).map_err(|error| worker_error(error.to_string()))?;
    if !remainder.is_empty() {
        return Err(worker_error("worker frame has trailing bytes"));
    }
    Ok(value)
}

pub(crate) fn read_bounded_frame(reader: &mut impl Read, max: usize) -> std::io::Result<Vec<u8>> {
    let mut prefix = [0; FRAME_LENGTH_BYTES];
    loop {
        match reader.read(&mut prefix[..1]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "worker stream closed between frames",
                ));
            }
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    reader.read_exact(&mut prefix[1..]).map_err(truncated_frame)?;
    let length = usize::try_from(u32::from_be_bytes(prefix)).expect("u32 frame length fits usize");
    if length == 0 || length > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "worker frame length is invalid",
        ));
    }
    let mut frame = vec![0; length];
    reader.read_exact(&mut frame).map_err(truncated_frame)?;
    Ok(frame)
}

fn truncated_frame(error: std::io::Error) -> std::io::Error {
    if error.kind() == std::io::ErrorKind::UnexpectedEof {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "worker frame is truncated")
    } else {
        error
    }
}

pub(super) struct BoundedBuffer {
    pub(super) bytes: Vec<u8>,
    max: usize,
}

impl BoundedBuffer {
    pub(super) fn new(max: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(max.min(8 * 1024)),
            max,
        }
    }
}

impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.max.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "worker frame exceeds maximum size",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn write_bounded_frame(writer: &mut impl Write, value: &impl Serialize, max: usize) -> Result<()> {
    let mut frame = BoundedBuffer::new(max);
    postcard::to_io(value, &mut frame).map_err(|error| worker_error(error.to_string()))?;
    let length = u32::try_from(frame.bytes.len()).map_err(|_| worker_error("worker frame length exceeds u32"))?;
    writer.write_all(&length.to_be_bytes()).map_err(worker_error)?;
    writer.write_all(&frame.bytes).map_err(worker_error)?;
    writer.flush().map_err(worker_error)
}

fn write_response(writer: &mut impl Write, id: u64, response: WorkerResponse) -> Result<()> {
    write_bounded_frame(writer, &ResponseFrame { id, response }, MAX_WORKER_FRAME_BYTES)
}

/// Terminate the worker as soon as the owning parent process disappears.
#[cfg(unix)]
#[allow(unsafe_code)]
fn install_parent_lifeline() -> Result<()> {
    use std::os::fd::{FromRawFd, OwnedFd};

    let Some(raw_fd) = std::env::var_os(super::PARENT_LIFELINE_FD_ENV) else {
        return Ok(());
    };
    let raw_fd = raw_fd
        .to_str()
        .ok_or_else(|| worker_error("parent lifeline descriptor is not valid UTF-8"))?
        .parse::<i32>()
        .map_err(|error| worker_error(format!("invalid parent lifeline descriptor: {error}")))?;
    if raw_fd < 0 {
        return Err(worker_error("parent lifeline descriptor is negative"));
    }
    // Duplicate instead of adopting the inherited descriptor: fcntl validates it
    // and returns a fresh descriptor, so a spoofed or already-owned value in the
    // environment can never be double-closed through OwnedFd.
    // SAFETY: fcntl(F_DUPFD_CLOEXEC) either fails or returns a new descriptor
    // whose sole owner is the OwnedFd constructed below.
    let duplicated = unsafe { libc::fcntl(raw_fd, libc::F_DUPFD_CLOEXEC, 3) };
    if duplicated < 0 {
        return Err(worker_error(std::io::Error::last_os_error()));
    }
    // SAFETY: `duplicated` was just created by fcntl and has independent ownership.
    let descriptor = unsafe { OwnedFd::from_raw_fd(duplicated) };
    std::thread::Builder::new()
        .name("servo-fetch-parent-lifeline".into())
        .spawn(move || {
            let mut pipe = std::fs::File::from(descriptor);
            let mut byte = [0];
            loop {
                match pipe.read(&mut byte) {
                    Ok(0) => {
                        // SAFETY: parent loss requires process-wide termination without Servo static destructors.
                        unsafe { libc::_exit(0) };
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        // SAFETY: a broken lifeline is equivalent to losing the owning parent.
                        unsafe { libc::_exit(1) };
                    }
                }
            }
        })
        .map_err(worker_error)?;
    Ok(())
}

#[cfg(not(unix))]
fn install_parent_lifeline() -> Result<()> {
    Ok(())
}

/// Reject a response frame whose id does not match the awaited request.
pub(crate) fn validate_response(response: &ResponseFrame, id: u64) -> Result<()> {
    if response.id != id {
        return Err(worker_error("worker response id mismatch"));
    }
    Ok(())
}

/// Run the isolated worker protocol before any Servo use.
pub fn run_worker_stdio() -> Result<()> {
    install_parent_lifeline()?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = BufWriter::new(stdout.lock());
    run_worker(&mut reader, &mut writer)
}

pub(super) fn run_worker<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<()> {
    write_bounded_frame(
        writer,
        &WorkerProtocolInfo {
            magic: WORKER_PROTOCOL_MAGIC,
            package_version: PACKAGE_VERSION.to_owned(),
        },
        MAX_WORKER_PROTOCOL_INFO_BYTES,
    )?;
    let mut state = WorkerState::AwaitingInitialize;
    loop {
        let bytes = match read_bounded_frame(reader, MAX_WORKER_REQUEST_FRAME_BYTES) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(worker_error(error)),
        };
        let request: RequestFrame = decode_frame(&bytes)?;
        match request.request {
            WorkerRequest::Initialize(config) => {
                let response = handle_worker_initialize(config, &mut state);
                write_response(writer, request.id, response)?;
            }
            WorkerRequest::Fetch(fetch) => handle_worker_fetch(request.id, fetch, &state, writer)?,
            WorkerRequest::Crawl(crawl) => handle_worker_crawl(request.id, crawl, &state, writer)?,
            WorkerRequest::Shutdown => {
                write_response(writer, request.id, WorkerResponse::ShutdownAck)?;
                return Ok(());
            }
        }
    }
}

pub(super) fn handle_worker_initialize(config: InitializeSession, state: &mut WorkerState) -> WorkerResponse {
    if !matches!(state, WorkerState::AwaitingInitialize) {
        let message = if matches!(state, WorkerState::Ready { .. }) {
            "worker session is already initialized"
        } else {
            "worker session initialization has already been attempted"
        };
        return WorkerResponse::failure("protocol", message);
    }
    let config = match ValidatedInitialize::from_wire(config) {
        Ok(config) => config,
        Err(error) => return WorkerResponse::from_error(&error),
    };
    let session_user_agent = config.user_agent.clone();

    // Mutating process-global Servo state makes any initialization failure terminal.
    *state = WorkerState::Failed;
    let result = crate::bridge::configure_engine_storage(config.config_dir, config.temporary_storage)
        .map_err(|error| worker_error(error.to_string()))
        .and_then(|()| {
            crate::bridge::try_set_engine_policy(config.policy).map_err(|error| worker_error(error.to_string()))
        })
        .and_then(|()| {
            crate::bridge::initialize_session(
                config.user_agent.as_deref(),
                config.cookie_scope.as_deref(),
                &config.cookies,
            )
            .map_err(|error| worker_error(error.to_string()))
        });
    match result {
        Ok(()) => {
            *state = WorkerState::Ready {
                user_agent: session_user_agent,
            };
            WorkerResponse::SessionInitialized
        }
        Err(error) => WorkerResponse::from_error(&error),
    }
}

pub(super) struct ValidatedInitialize {
    pub(crate) policy: NetworkPolicy,
    pub(crate) user_agent: Option<String>,
    cookies: Vec<crate::cookies::CookieSpec>,
    pub(crate) cookie_scope: Option<String>,
    config_dir: PathBuf,
    temporary_storage: bool,
}

impl ValidatedInitialize {
    const MAX_USER_AGENT_BYTES: usize = 8 * 1024;

    pub(crate) fn from_wire(config: InitializeSession) -> Result<Self> {
        let policy = if config.permissive_network {
            NetworkPolicy::PERMISSIVE
        } else {
            NetworkPolicy::STRICT
        };
        let user_agent = config
            .user_agent
            .map(|user_agent| {
                if user_agent.len() > Self::MAX_USER_AGENT_BYTES {
                    return Err(worker_error(format!(
                        "worker session user-agent exceeds {} bytes",
                        Self::MAX_USER_AGENT_BYTES
                    )));
                }
                Ok(crate::net::sanitize_user_agent(user_agent))
            })
            .transpose()?;
        let cookies = CookieWire::into_specs(config.cookies).map_err(worker_error)?;
        if !cookies.is_empty() && config.cookie_scope.is_none() {
            return Err(worker_error(
                "cookie_scope is required when session cookies are configured",
            ));
        }
        let cookie_scope = config
            .cookie_scope
            .map(|scope| {
                crate::net::validate_url_with_policy(&scope, policy)
                    .map(|url| url.to_string())
                    .map_err(|error| crate::error::map_url_error(&scope, error))
            })
            .transpose()?;
        if !config.config_dir.is_absolute() {
            return Err(worker_error("worker config_dir must be absolute"));
        }
        if !config.config_dir.is_dir() {
            return Err(worker_error("worker config_dir must be an existing directory"));
        }
        Ok(Self {
            policy,
            user_agent,
            cookies,
            cookie_scope,
            config_dir: config.config_dir,
            temporary_storage: config.temporary_storage,
        })
    }
}

fn handle_worker_fetch(id: u64, fetch: FetchWire, state: &WorkerState, writer: &mut impl Write) -> Result<()> {
    if !matches!(state, WorkerState::Ready { .. }) {
        return write_response(
            writer,
            id,
            WorkerResponse::failure("protocol", "worker session is not initialized"),
        );
    }
    let result = fetch
        .into_options()
        .and_then(|opts| crate::fetch::fetch_in_process_blocking(&opts))
        .and_then(PageWire::from_page);
    let (page, screenshot) = match result {
        Ok(result) => result,
        Err(error) => return write_response(writer, id, WorkerResponse::from_error(&error)),
    };
    write_response(writer, id, WorkerResponse::FetchResult(page))?;
    if let Some(screenshot) = screenshot {
        for chunk in screenshot.chunks(MAX_WORKER_BLOB_CHUNK_BYTES) {
            write_response(writer, id, WorkerResponse::ScreenshotChunk(chunk.to_vec()))?;
        }
    }
    write_response(writer, id, WorkerResponse::FetchCompleted)
}

#[derive(Default)]
pub(crate) struct CrawlProgressState {
    processed: u64,
    emitted: u64,
    suppressed: u64,
}

impl CrawlProgressState {
    pub(crate) fn observe(&mut self, event: crate::crawl::CrawlSessionEvent) -> WorkerResponse {
        self.processed = self.processed.saturating_add(1);
        match event {
            crate::crawl::CrawlSessionEvent::Result(result) => {
                self.emitted = self.emitted.saturating_add(1);
                WorkerResponse::CrawlResult(CrawlResultWire::from_result(result))
            }
            crate::crawl::CrawlSessionEvent::Suppressed(page) => {
                self.suppressed = self.suppressed.saturating_add(1);
                tracing::debug!(
                    url = %page.url,
                    depth = page.depth,
                    reason = ?page.reason,
                    "crawl result suppressed"
                );
                WorkerResponse::CrawlProgress(CrawlProgressWire {
                    processed: self.processed,
                    emitted: self.emitted,
                    suppressed: self.suppressed,
                })
            }
        }
    }
}

fn handle_worker_crawl(id: u64, crawl: CrawlWire, state: &WorkerState, writer: &mut impl Write) -> Result<()> {
    let WorkerState::Ready { user_agent } = state else {
        return write_response(
            writer,
            id,
            WorkerResponse::failure("protocol", "worker session is not initialized"),
        );
    };
    let outcome = crawl.into_options(user_agent.as_deref()).and_then(|opts| {
        let mut progress = CrawlProgressState::default();
        crate::crawl::crawl_each_in_process_blocking_with_events(&opts, |event| {
            write_response(writer, id, progress.observe(event))
        })
    });
    let terminal = match outcome {
        Ok(()) => WorkerResponse::CrawlCompleted,
        Err(error) => WorkerResponse::from_error(&error),
    };
    write_response(writer, id, terminal)
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum WorkerState {
    AwaitingInitialize,
    Ready { user_agent: Option<String> },
    Failed,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct WorkerProtocolInfo {
    pub(crate) magic: [u8; 8],
    pub(crate) package_version: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct InitializeSession {
    pub(crate) permissive_network: bool,
    pub(crate) user_agent: Option<String>,
    pub(crate) cookies: Vec<CookieWire>,
    pub(crate) cookie_scope: Option<String>,
    pub(crate) config_dir: PathBuf,
    pub(crate) temporary_storage: bool,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RequestFrame {
    pub(crate) id: u64,
    pub(crate) request: WorkerRequest,
}

#[derive(Serialize, Deserialize)]
pub(crate) enum WorkerRequest {
    Initialize(InitializeSession),
    Fetch(FetchWire),
    Crawl(CrawlWire),
    Shutdown,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ResponseFrame {
    pub(crate) id: u64,
    pub(crate) response: WorkerResponse,
}

#[derive(Serialize, Deserialize)]
pub(crate) enum WorkerResponse {
    SessionInitialized,
    FetchResult(PageWire),
    ScreenshotChunk(Vec<u8>),
    FetchCompleted,
    CrawlResult(CrawlResultWire),
    CrawlProgress(CrawlProgressWire),
    CrawlCompleted,
    ShutdownAck,
    Error(WorkerErrorWire),
}

impl WorkerResponse {
    fn from_error(error: &Error) -> Self {
        Self::Error(WorkerErrorWire::from_error(error))
    }

    fn failure(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error(WorkerErrorWire::failure(kind, message))
    }
}
