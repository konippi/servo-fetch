//! Worker process ownership, lifecycle supervision, and resource cleanup.

use std::io::{BufReader, BufWriter};
#[cfg(unix)]
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use tokio::sync::OwnedSemaphorePermit;

use super::{SessionCancellation, WorkerCommand, cancelled_error, is_terminal_session_error};
use crate::CrawlResult;
use crate::error::{Error, Result};
#[cfg(windows)]
use crate::sys::windows::{resume_suspended_process, suspend_new_process};
use crate::worker::protocol::{
    InitializeSession, PACKAGE_VERSION, RequestFrame, ResponseFrame, WorkerProtocolInfo, WorkerRequest, WorkerResponse,
    decode_frame, read_bounded_frame, validate_response, write_bounded_frame,
};
use crate::worker::wire::{
    CrawlWire, FetchWire, MAX_SCREENSHOT_BYTES, PageWire, crawl_wire_absolute_watchdog, crawl_wire_watchdog,
    fetch_wire_watchdog,
};
use crate::worker::{
    MAX_WORKER_BLOB_CHUNK_BYTES, MAX_WORKER_FRAME_BYTES, MAX_WORKER_PROTOCOL_INFO_BYTES,
    MAX_WORKER_REQUEST_FRAME_BYTES, WORKER_PROTOCOL_MAGIC, worker_error,
};

mod kill;
mod process_tree;

use kill::KillAuthority;
#[cfg(unix)]
pub(in crate::session) use process_tree::create_parent_lifeline;
#[cfg(unix)]
use process_tree::inherit_parent_lifeline;
use process_tree::{ProcessTree, observe_child_exit_without_reaping};

const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(10);
const REAP_GRACE: Duration = Duration::from_secs(2);
#[derive(Debug, Clone, Copy)]
enum Lifecycle {
    Force,
}

type ProcessTreeAuthority = Arc<KillAuthority<ProcessTree>>;

/// Kills the worker process tree directly so a stuck supervisor cannot block cancellation.
#[derive(Debug, Clone)]
pub(super) struct ForcePort {
    lifecycle: Sender<Lifecycle>,
    authority: ProcessTreeAuthority,
}

impl ForcePort {
    pub(super) fn force(&self) {
        // Coalesce the supervisor notification while still issuing a prompt kill.
        let _ = self.lifecycle.try_send(Lifecycle::Force);
        self.authority.terminate_now();
    }
}

pub(super) type ResponseSender<T> = Sender<Result<T>>;
pub(super) type ResponseReceiver<T> = Receiver<Result<T>>;

pub(super) fn frame_wait_duration(idle_timeout: Duration, remaining: Duration) -> Option<Duration> {
    (!remaining.is_zero()).then(|| idle_timeout.min(remaining))
}

pub(super) fn response_channel<T>() -> (ResponseSender<T>, ResponseReceiver<T>) {
    crossbeam_channel::bounded(1)
}

pub(super) enum SupervisorCommand {
    Initialize {
        request: InitializeSession,
        reply: ResponseSender<()>,
    },
    Fetch {
        request: FetchWire,
        reply: ResponseSender<(PageWire, Option<Vec<u8>>)>,
    },
    Crawl {
        request: CrawlWire,
        events: Sender<CrawlResult>,
        reply: ResponseSender<()>,
    },
    Shutdown {
        reply: ResponseSender<()>,
    },
}

pub(super) struct SupervisorHandle {
    commands: Sender<SupervisorCommand>,
    pub(super) force_port: ForcePort,
    terminal: Option<ResponseReceiver<()>>,
    pub(super) config_dir: PathBuf,
    armed: bool,
}

impl std::fmt::Debug for SupervisorHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SupervisorHandle")
            .field("config_dir", &self.config_dir)
            .field("armed", &self.armed)
            .finish_non_exhaustive()
    }
}

impl SupervisorHandle {
    pub(super) fn spawn(
        command: WorkerCommand,
        permit: OwnedSemaphorePermit,
        cancellation: Option<&SessionCancellation>,
    ) -> Result<Self> {
        let (commands, command_rx) = crossbeam_channel::bounded(1);
        let (lifecycle, lifecycle_rx) = crossbeam_channel::bounded(1);
        let (bootstrap_result, bootstrap_result_rx) = response_channel();
        let (terminal, terminal_rx) = response_channel();
        let force_port = ForcePort {
            lifecycle,
            authority: Arc::new(KillAuthority::new()),
        };
        if cancellation.is_some_and(|cancel| !cancel.attach(&force_port)) {
            return Err(cancelled_error());
        }
        let authority = Arc::clone(&force_port.authority);
        std::thread::Builder::new()
            .name("servo-fetch-supervisor".into())
            .spawn(move || {
                let result = supervisor_thread(command, permit, command_rx, lifecycle_rx, authority, bootstrap_result);
                if let Err(send_error) = terminal.send(result)
                    && let Err(error) = send_error.into_inner()
                {
                    tracing::error!(%error, "detached supervisor cleanup failed");
                }
            })
            .map_err(worker_error)?;
        let config_dir = receive_response(&bootstrap_result_rx, "worker protocol bootstrap")?;
        Ok(Self {
            commands,
            force_port,
            terminal: Some(terminal_rx),
            config_dir,
            armed: true,
        })
    }

    pub(super) fn send(&self, command: SupervisorCommand) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| worker_error("worker supervisor stopped"))
    }

    pub(super) fn initialize_session(&mut self, request: InitializeSession) -> Result<()> {
        let (reply, receive) = response_channel();
        self.send(SupervisorCommand::Initialize { request, reply })?;
        receive_response(&receive, "worker session initialization")
    }

    pub(super) fn begin_close(&mut self) -> Result<ResponseReceiver<()>> {
        let (reply, receive) = response_channel();
        self.send(SupervisorCommand::Shutdown { reply })?;
        Ok(receive)
    }

    pub(super) fn close_blocking(&mut self) -> Result<()> {
        let receive = self.begin_close()?;
        receive_response(&receive, "session close")
    }

    pub(super) fn begin_force(&mut self) -> Result<ResponseReceiver<()>> {
        self.force();
        self.terminal
            .take()
            .ok_or_else(|| worker_error("supervisor terminal receiver unavailable"))
    }

    pub(super) fn force(&mut self) {
        if self.armed {
            self.force_port.force();
            self.armed = false;
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SupervisorHandle {
    fn drop(&mut self) {
        // Drop only enqueues force-close; the supervisor owns teardown.
        self.force();
    }
}

pub(super) async fn recv_optional_async<T: Send + 'static>(receive: Receiver<T>) -> Result<Option<T>> {
    tokio::task::spawn_blocking(move || receive.recv().ok())
        .await
        .map_err(|error| worker_error(format!("crawl event task failed: {error}")))
}

pub(super) async fn recv_async<T: Send + 'static>(receive: ResponseReceiver<T>, context: &'static str) -> Result<T> {
    tokio::task::spawn_blocking(move || receive_response(&receive, context))
        .await
        .map_err(|error| worker_error(format!("{context} task failed: {error}")))?
}

pub(super) fn receive_response<T>(receive: &ResponseReceiver<T>, context: &str) -> Result<T> {
    receive
        .recv()
        .map_err(|_| worker_error(format!("{context} response channel closed")))?
}

pub(super) struct SupervisorOwner {
    config_dir: Option<PathBuf>,
    permit: Option<OwnedSemaphorePermit>,
}

impl SupervisorOwner {
    pub(super) fn new(permit: OwnedSemaphorePermit) -> Result<Self> {
        let temp_dir = tempfile::Builder::new()
            .prefix("servo-fetch-session-")
            .tempdir()
            .map_err(worker_error)?;
        super::scavenge::write_owner_marker(temp_dir.path());
        Ok(Self {
            config_dir: Some(temp_dir.keep()),
            permit: Some(permit),
        })
    }

    pub(super) fn config_dir(&self) -> PathBuf {
        self.config_dir.as_ref().expect("supervisor tempdir is present").clone()
    }

    fn release_with(&mut self, mut remove: impl FnMut(&std::path::Path) -> std::io::Result<()>) -> Result<()> {
        if let Some(path) = self.config_dir.as_deref() {
            let mut last_error = None;
            for _ in 0..3 {
                match remove(path) {
                    Ok(()) => {
                        last_error = None;
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        last_error = None;
                        break;
                    }
                    Err(error) => {
                        last_error = Some(error);
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            if let Some(error) = last_error {
                return Err(worker_error(format!(
                    "failed to delete browser session storage at {}: {error}",
                    path.display()
                )));
            }
            self.config_dir.take();
        }
        self.permit.take();
        Ok(())
    }

    pub(super) fn release(&mut self) -> Result<()> {
        self.release_with(|path| std::fs::remove_dir_all(path))
    }
}

impl Drop for SupervisorOwner {
    fn drop(&mut self) {
        if let Err(error) = self.release() {
            if let Some(permit) = self.permit.take() {
                std::mem::forget(permit);
            }
            tracing::error!(%error, "supervisor owner cleanup failed; retaining broker capacity");
        }
    }
}

fn cleanup_spawned_child(
    child: &mut Child,
    process_tree: &ProcessTree,
    authority: &ProcessTreeAuthority,
) -> Result<()> {
    drop(authority.revoke());
    process_tree.terminate(child).map_err(worker_error)?;
    child.wait().map_err(worker_error)?;
    #[cfg(windows)]
    process_tree.wait_quiescent().map_err(worker_error)?;
    Ok(())
}

struct WorkerProcess {
    child: Option<Child>,
    process_tree: Arc<ProcessTree>,
    authority: ProcessTreeAuthority,
    stdin: Option<BufWriter<ChildStdin>>,
    frames: Option<Receiver<std::io::Result<Vec<u8>>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    #[cfg(unix)]
    parent_lifeline: Option<OwnedFd>,
    owner: SupervisorOwner,
    lifecycle: Receiver<Lifecycle>,
    termination_started: bool,
    exit_status: Option<ExitStatus>,
    quiescent: bool,
    next_id: u64,
}

impl WorkerProcess {
    fn spawn(
        command: &WorkerCommand,
        permit: OwnedSemaphorePermit,
        lifecycle: Receiver<Lifecycle>,
        authority: ProcessTreeAuthority,
    ) -> Result<Self> {
        command.validate()?;
        if lifecycle.try_recv().is_ok() {
            return Err(cancelled_error());
        }
        let owner = SupervisorOwner::new(permit)?;
        #[cfg(unix)]
        let (lifeline_read, lifeline_write) = create_parent_lifeline()?;
        let mut process = Command::new(&command.program);
        process
            .args(&command.args)
            .current_dir(owner.config_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(unix)]
        inherit_parent_lifeline(&mut process, lifeline_read.as_raw_fd());
        #[cfg(windows)]
        suspend_new_process(&mut process);
        let mut child = process.spawn().map_err(worker_error)?;
        #[cfg(unix)]
        drop(lifeline_read);

        let process_tree = match ProcessTree::attach(&child).map(Arc::new) {
            Ok(process_tree) => process_tree,
            Err(error) => {
                let cleanup = child.kill().and_then(|()| child.wait().map(drop)).map_err(worker_error);
                return with_cleanup(Err(error), cleanup);
            }
        };
        authority.grant(Arc::clone(&process_tree));
        let fail = |child: &mut Child, error: Error| -> Result<Self> {
            with_cleanup(Err(error), cleanup_spawned_child(child, &process_tree, &authority))
        };

        if lifecycle.try_recv().is_ok() {
            return fail(&mut child, cancelled_error());
        }
        #[cfg(windows)]
        if let Err(error) = resume_suspended_process(&child).map_err(worker_error) {
            return fail(&mut child, error);
        }

        let Some(stdin) = child.stdin.take() else {
            return fail(&mut child, worker_error("worker stdin unavailable"));
        };
        let Some(stdout) = child.stdout.take() else {
            return fail(&mut child, worker_error("worker stdout unavailable"));
        };
        let (frames_tx, frames) = crossbeam_channel::bounded(1);
        let reader = std::thread::Builder::new()
            .name("servo-fetch-worker-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut first_frame = true;
                loop {
                    let max = if first_frame {
                        MAX_WORKER_PROTOCOL_INFO_BYTES
                    } else {
                        MAX_WORKER_FRAME_BYTES
                    };
                    let frame = read_bounded_frame(&mut reader, max);
                    if frame.is_ok() {
                        first_frame = false;
                    }
                    let done = frame
                        .as_ref()
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::UnexpectedEof);
                    if frames_tx.send(frame).is_err() || done {
                        break;
                    }
                }
            });
        let reader = match reader {
            Ok(reader) => reader,
            Err(error) => return fail(&mut child, worker_error(error)),
        };
        Ok(Self {
            child: Some(child),
            process_tree,
            authority,
            stdin: Some(BufWriter::new(stdin)),
            frames: Some(frames),
            reader: Some(reader),
            #[cfg(unix)]
            parent_lifeline: Some(lifeline_write),
            owner,
            lifecycle,
            termination_started: false,
            exit_status: None,
            quiescent: false,
            next_id: 1,
        })
    }

    fn config_dir(&self) -> PathBuf {
        self.owner.config_dir()
    }

    fn receive_protocol_info(&mut self) -> Result<()> {
        let deadline = Instant::now() + BOOTSTRAP_TIMEOUT;
        let bytes = self.recv_frame(BOOTSTRAP_TIMEOUT, deadline, BOOTSTRAP_TIMEOUT, "protocol info")?;
        let info: WorkerProtocolInfo = decode_frame(&bytes)?;
        if info.magic != WORKER_PROTOCOL_MAGIC {
            return Err(worker_error(format!(
                "worker protocol mismatch: expected magic {WORKER_PROTOCOL_MAGIC:?}; got {:?}",
                info.magic
            )));
        }
        // Postcard encoding is not forward-compatible, so a worker built from a
        // different package version must be rejected before any request frame.
        if info.package_version != PACKAGE_VERSION {
            return Err(worker_error(format!(
                "worker package version mismatch: parent {PACKAGE_VERSION}, worker {}",
                info.package_version
            )));
        }
        Ok(())
    }

    fn initialize_session(&mut self, initialization: InitializeSession) -> Result<()> {
        let id = self.write_request(WorkerRequest::Initialize(initialization))?;
        match self.recv_response(id, BOOTSTRAP_TIMEOUT, "session initialization")? {
            WorkerResponse::SessionInitialized => Ok(()),
            WorkerResponse::Error(error) => Err(error.into_error()),
            _ => Err(worker_error("unexpected session initialization response")),
        }
    }

    fn fetch(&mut self, request: FetchWire) -> Result<(PageWire, Option<Vec<u8>>)> {
        let timeout = fetch_wire_watchdog(&request);
        let deadline = Instant::now() + timeout;
        let id = self.write_request(WorkerRequest::Fetch(request))?;
        let mut page = None;
        let mut screenshot = None;
        loop {
            let response = self.recv_response_until(id, timeout, deadline, timeout, "fetch")?;
            match response {
                WorkerResponse::FetchResult(result) if page.is_none() => {
                    let declared_size = result.screenshot_png_bytes();
                    if declared_size.is_some_and(|size| size > MAX_SCREENSHOT_BYTES) {
                        return Err(worker_error("declared screenshot payload exceeds maximum size"));
                    }
                    screenshot = declared_size.map(Vec::with_capacity);
                    page = Some(result);
                }
                WorkerResponse::ScreenshotChunk(chunk) => {
                    if chunk.is_empty() || chunk.len() > MAX_WORKER_BLOB_CHUNK_BYTES {
                        return Err(worker_error("invalid screenshot chunk size"));
                    }
                    let expected = page
                        .as_ref()
                        .and_then(PageWire::screenshot_png_bytes)
                        .ok_or_else(|| worker_error("unexpected screenshot chunk"))?;
                    let target = screenshot
                        .as_mut()
                        .ok_or_else(|| worker_error("screenshot payload was not declared"))?;
                    if chunk.len() > expected.saturating_sub(target.len()) {
                        return Err(worker_error("screenshot payload exceeds declared size"));
                    }
                    target.extend_from_slice(&chunk);
                }
                WorkerResponse::FetchCompleted => {
                    let page = page.ok_or_else(|| worker_error("fetch completed without a result"))?;
                    if screenshot.as_ref().map(Vec::len) != page.screenshot_png_bytes() {
                        return Err(worker_error("incomplete screenshot payload"));
                    }
                    return Ok((page, screenshot));
                }
                WorkerResponse::Error(error) if page.is_none() => return Err(error.into_error()),
                WorkerResponse::Error(_) => {
                    return Err(worker_error("worker returned an error after a partial fetch response"));
                }
                _ => return Err(worker_error("unexpected fetch response")),
            }
        }
    }

    fn crawl(&mut self, request: CrawlWire, events: &Sender<CrawlResult>) -> Result<()> {
        let idle_timeout = crawl_wire_watchdog(&request);
        let absolute_timeout = crawl_wire_absolute_watchdog(&request);
        let deadline = Instant::now() + absolute_timeout;
        let id = self.write_request(WorkerRequest::Crawl(request))?;
        loop {
            match self.recv_response_until(id, idle_timeout, deadline, absolute_timeout, "crawl")? {
                WorkerResponse::CrawlResult(event) => {
                    let result = event.into_result()?;
                    crossbeam_channel::select_biased! {
                        recv(self.lifecycle) -> _ => return Err(cancelled_error()),
                        send(events, result) -> sent => {
                            sent.map_err(|_| worker_error("crawl result receiver closed"))?;
                        },
                    }
                }
                WorkerResponse::CrawlProgress(_) => {}
                WorkerResponse::CrawlCompleted => return Ok(()),
                WorkerResponse::Error(error) => return Err(error.into_error()),
                _ => return Err(worker_error("unexpected crawl response")),
            }
        }
    }

    fn graceful_shutdown(&mut self) -> Result<()> {
        let id = self.write_request(WorkerRequest::Shutdown)?;
        match self.recv_response(id, REAP_GRACE, "shutdown")? {
            WorkerResponse::ShutdownAck => {}
            WorkerResponse::Error(error) => return Err(error.into_error()),
            _ => return Err(worker_error("unexpected shutdown response")),
        }
        self.stdin.take();
        let deadline = Instant::now() + REAP_GRACE;
        loop {
            let child = self.child.as_mut().expect("worker child is present");
            #[cfg(any(unix, windows))]
            let exited = observe_child_exit_without_reaping(child).map_err(worker_error)?;
            #[cfg(not(any(unix, windows)))]
            let exited = false;
            if exited {
                break;
            }
            if Instant::now() >= deadline {
                return Err(worker_error("worker did not exit after shutdown acknowledgement"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let status = self.terminate_and_reap()?;
        if status.success() {
            Ok(())
        } else {
            Err(worker_error(format!("worker exited with {status} after shutdown ack")))
        }
    }

    fn terminate_and_reap(&mut self) -> Result<ExitStatus> {
        if !self.termination_started {
            // Revocation serializes with any in-flight cancellation kill.
            drop(self.authority.revoke());
            let child = self.child.as_ref().expect("worker child is present");
            self.process_tree.terminate(child).map_err(worker_error)?;
            self.termination_started = true;
        }
        if self.exit_status.is_none() {
            let status = self
                .child
                .as_mut()
                .expect("worker child is present")
                .wait()
                .map_err(worker_error)?;
            self.exit_status = Some(status);
        }
        #[cfg(windows)]
        self.process_tree.wait_quiescent().map_err(worker_error)?;
        self.quiescent = true;
        Ok(self.exit_status.expect("reaped worker has an exit status"))
    }

    fn write_request(&mut self, request: WorkerRequest) -> Result<u64> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let frame = RequestFrame { id, request };
        write_bounded_frame(
            self.stdin.as_mut().ok_or_else(|| worker_error("worker stdin closed"))?,
            &frame,
            MAX_WORKER_REQUEST_FRAME_BYTES,
        )?;
        Ok(id)
    }

    fn recv_response(&self, id: u64, timeout: Duration, operation: &'static str) -> Result<WorkerResponse> {
        let deadline = Instant::now() + timeout;
        self.recv_response_until(id, timeout, deadline, timeout, operation)
    }

    fn recv_response_until(
        &self,
        id: u64,
        idle_timeout: Duration,
        absolute_deadline: Instant,
        absolute_timeout: Duration,
        operation: &'static str,
    ) -> Result<WorkerResponse> {
        let bytes = self.recv_frame(idle_timeout, absolute_deadline, absolute_timeout, operation)?;
        let response: ResponseFrame = decode_frame(&bytes)?;
        validate_response(&response, id)?;
        Ok(response.response)
    }

    fn recv_frame(
        &self,
        idle_timeout: Duration,
        absolute_deadline: Instant,
        absolute_timeout: Duration,
        operation: &'static str,
    ) -> Result<Vec<u8>> {
        let remaining = absolute_deadline.saturating_duration_since(Instant::now());
        let Some(wait) = frame_wait_duration(idle_timeout, remaining) else {
            return Err(Error::WorkerProtocolTimeout {
                operation,
                timeout: absolute_timeout,
            });
        };
        let frames = self.frames.as_ref().expect("worker frame receiver is present");
        let timer = crossbeam_channel::after(wait);
        crossbeam_channel::select_biased! {
            recv(self.lifecycle) -> _ => Err(cancelled_error()),
            recv(frames) -> frame => match frame {
                Ok(Ok(frame)) => Ok(frame),
                Ok(Err(error)) => Err(worker_error(error)),
                Err(_) => Err(worker_error("worker transport closed")),
            },
            recv(timer) -> _ => {
                let timeout = if Instant::now() >= absolute_deadline {
                    absolute_timeout
                } else {
                    idle_timeout
                };
                Err(Error::WorkerProtocolTimeout { operation, timeout })
            },
        }
    }

    fn cleanup(&mut self, force: bool) -> Result<()> {
        self.stdin.take();
        #[cfg(unix)]
        self.parent_lifeline.take();

        if !self.quiescent {
            if !force {
                return Err(worker_error(
                    "worker process tree was not quiescent after graceful shutdown",
                ));
            }
            self.terminate_and_reap()?;
        }
        self.child.take();
        self.frames.take();

        let reader = match self.reader.take().map(std::thread::JoinHandle::join) {
            Some(Err(_)) => Err(worker_error("worker stdout reader panicked")),
            _ => Ok(()),
        };
        with_cleanup(reader, self.owner.release())
    }
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup(true) {
            tracing::error!(%error, "best-effort worker cleanup failed");
        }
    }
}

fn with_cleanup<T>(primary: Result<T>, cleanup: Result<()>) -> Result<T> {
    match (primary, cleanup) {
        (primary, Ok(())) => primary,
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(error), Err(cleanup)) => Err(worker_error(format!("{error}; cleanup failed: {cleanup}"))),
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the supervisor thread owns command channels and worker configuration for its lifetime"
)]
fn supervisor_thread(
    command: WorkerCommand,
    permit: OwnedSemaphorePermit,
    commands: Receiver<SupervisorCommand>,
    lifecycle: Receiver<Lifecycle>,
    authority: ProcessTreeAuthority,
    bootstrap_result: ResponseSender<PathBuf>,
) -> Result<()> {
    let mut worker = match WorkerProcess::spawn(&command, permit, lifecycle.clone(), authority) {
        Ok(worker) => worker,
        Err(error) => {
            let _ = bootstrap_result.send(Err(error));
            return Ok(());
        }
    };
    if let Err(error) = worker.receive_protocol_info() {
        let result = with_cleanup(Err(error), worker.cleanup(true));
        let _ = bootstrap_result.send(result.map(|()| worker.config_dir()));
        return Ok(());
    }
    if bootstrap_result.send(Ok(worker.config_dir())).is_err() {
        return worker.cleanup(true);
    }

    loop {
        crossbeam_channel::select_biased! {
            recv(lifecycle) -> _ => return worker.cleanup(true),
            recv(commands) -> command => {
                let Ok(command) = command else {
                    return worker.cleanup(true);
                };
                match command {
                    SupervisorCommand::Initialize { request, reply } => match worker.initialize_session(request) {
                        Ok(()) => {
                            let _ = reply.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = reply.send(with_cleanup(Err(error), worker.cleanup(true)));
                            return Ok(());
                        }
                    },
                    SupervisorCommand::Fetch { request, reply } => match worker.fetch(request) {
                        Ok(page) => {
                            let _ = reply.send(Ok(page));
                        }
                        Err(error) => {
                            let terminal = is_terminal_session_error(&error);
                            let result = if terminal {
                                with_cleanup(Err(error), worker.cleanup(true))
                            } else {
                                Err(error)
                            };
                            let _ = reply.send(result);
                            if terminal {
                                return Ok(());
                            }
                        }
                    },
                    SupervisorCommand::Crawl { request, events, reply } => {
                        let result = worker.crawl(request, &events);
                        drop(events);
                        match result {
                            Ok(()) => {
                                let _ = reply.send(Ok(()));
                            }
                            Err(error) => {
                                let terminal = is_terminal_session_error(&error);
                                let result = if terminal {
                                    with_cleanup(Err(error), worker.cleanup(true))
                                } else {
                                    Err(error)
                                };
                                let _ = reply.send(result);
                                if terminal {
                                    return Ok(());
                                }
                            }
                        }
                    }
                    SupervisorCommand::Shutdown { reply } => {
                        let result = worker.graceful_shutdown();
                        let graceful = result.is_ok();
                        let result = with_cleanup(result, worker.cleanup(!graceful));
                        let _ = reply.send(result);
                        return Ok(());
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_failure_is_returned_without_releasing_permit() {
        let permits = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = permits.clone().try_acquire_owned().unwrap();
        let mut owner = SupervisorOwner::new(permit).unwrap();

        let error = owner
            .release_with(|_| Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "injected")))
            .unwrap_err();
        assert!(error.to_string().contains("injected"));
        assert!(permits.clone().try_acquire_owned().is_err());

        owner.release().unwrap();
        assert!(permits.try_acquire_owned().is_ok());
    }
}
