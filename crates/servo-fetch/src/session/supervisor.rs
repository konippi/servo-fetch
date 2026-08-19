//! Worker process ownership, lifecycle supervision, and resource cleanup.

use std::io::{BufReader, BufWriter};
#[cfg(unix)]
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle as _;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use tokio::sync::OwnedSemaphorePermit;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    TerminateJobObject,
};

use super::{SessionCancellation, WorkerCommand, cancelled_error, is_terminal_session_error};
use crate::CrawlResult;
use crate::error::{Error, Result};
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
    MAX_WORKER_REQUEST_FRAME_BYTES, PARENT_LIFELINE_FD_ENV, WORKER_PROTOCOL_MAGIC, worker_error,
};

const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(10);
const REAP_GRACE: Duration = Duration::from_secs(2);
const READER_SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
#[derive(Debug, Clone, Copy)]
enum Lifecycle {
    Force,
}

type ProcessTreeSlot = Arc<OnceLock<Arc<ProcessTree>>>;

/// Force-close port that both notifies the supervisor and kills the worker
/// process tree directly, so cancellation cannot be blocked by a supervisor
/// thread stuck on pipe I/O.
#[derive(Debug, Clone)]
pub(super) struct ForcePort {
    lifecycle: Sender<Lifecycle>,
    process_tree: ProcessTreeSlot,
}

impl ForcePort {
    pub(super) fn force(&self) {
        // try_send coalesces repeated cancels: one pending Force is sufficient.
        let _ = self.lifecycle.try_send(Lifecycle::Force);
        if let Some(tree) = self.process_tree.get() {
            tree.terminate_now();
        }
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

#[cfg(unix)]
#[allow(unsafe_code)]
pub(super) fn create_parent_lifeline() -> Result<(OwnedFd, OwnedFd)> {
    let (child_end, parent_end) = std::os::unix::net::UnixStream::pair().map_err(worker_error)?;
    let duplicate = |descriptor: OwnedFd| -> Result<OwnedFd> {
        // F_DUPFD_CLOEXEC atomically creates a descriptor outside the standard streams.
        // SAFETY: descriptor is valid and the returned descriptor has independent ownership.
        let duplicated = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if duplicated < 0 {
            return Err(worker_error(std::io::Error::last_os_error()));
        }
        // SAFETY: fcntl returned a new descriptor whose ownership is transferred here.
        Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
    };
    Ok((duplicate(child_end.into())?, duplicate(parent_end.into())?))
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn inherit_parent_lifeline(process: &mut Command, lifeline_fd: i32) {
    process
        .process_group(0)
        .env(PARENT_LIFELINE_FD_ENV, lifeline_fd.to_string());
    // SAFETY: fcntl is async-signal-safe and only clears CLOEXEC on the dedicated
    // descriptor after fork, so concurrent process spawns cannot inherit it.
    unsafe {
        process.pre_exec(move || {
            let flags = libc::fcntl(lifeline_fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(lifeline_fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
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
            process_tree: ProcessTreeSlot::default(),
        };
        if cancellation.is_some_and(|cancel| !cancel.attach(&force_port)) {
            return Err(cancelled_error());
        }
        let tree_slot = force_port.process_tree.clone();
        std::thread::Builder::new()
            .name("servo-fetch-supervisor".into())
            .spawn(move || {
                supervisor_thread(command, permit, command_rx, lifecycle_rx, tree_slot, bootstrap_result);
                let _ = terminal.send(Ok(()));
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
    temp_dir: Option<tempfile::TempDir>,
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
            temp_dir: Some(temp_dir),
            permit: Some(permit),
        })
    }

    pub(super) fn config_dir(&self) -> PathBuf {
        self.temp_dir
            .as_ref()
            .expect("supervisor tempdir is present")
            .path()
            .to_path_buf()
    }

    pub(super) fn release(&mut self) {
        if let Some(temp_dir) = self.temp_dir.take() {
            let path = temp_dir.path().to_path_buf();
            drop(temp_dir);
            for _ in 0..3 {
                if !path.exists() || std::fs::remove_dir_all(&path).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if path.exists() {
                tracing::error!(path = %path.display(), "failed to delete browser session storage");
            }
        }
        self.permit.take();
    }
}

impl Drop for SupervisorOwner {
    fn drop(&mut self) {
        self.release();
    }
}

#[derive(Debug)]
struct ProcessTree {
    #[cfg(windows)]
    job: WindowsJob,
    #[cfg(unix)]
    process_group: Option<i32>,
    #[cfg(not(any(unix, windows)))]
    child_id: u32,
}

impl ProcessTree {
    #[cfg(windows)]
    fn attach(child: &Child) -> Result<Self> {
        Ok(Self {
            job: WindowsJob::attach(child)?,
        })
    }

    #[cfg(not(windows))]
    fn attach(child: &Child) -> Self {
        #[cfg(unix)]
        {
            Self {
                process_group: i32::try_from(child.id()).ok(),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self { child_id: child.id() }
        }
    }

    fn terminate(&self, child: &Child) {
        let child_id = child.id();
        #[cfg(unix)]
        debug_assert_eq!(
            i32::try_from(child_id).ok(),
            self.process_group,
            "worker process group must match the direct child"
        );
        #[cfg(windows)]
        debug_assert_ne!(child_id, 0, "worker child must have a process ID");
        #[cfg(not(any(unix, windows)))]
        debug_assert_eq!(self.child_id, child_id, "worker child identity must remain stable");
        self.terminate_now();
    }

    fn terminate_now(&self) {
        #[cfg(unix)]
        if let Some(group) = self.process_group {
            // SAFETY: this supervisor exclusively owns the child's dedicated process group.
            #[allow(unsafe_code)]
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        self.job.terminate();
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct WindowsJob(HANDLE);

// SAFETY: job object handles are process-wide kernel objects; TerminateJobObject
// and CloseHandle are documented as callable from any thread.
#[cfg(windows)]
#[allow(unsafe_code)]
unsafe impl Send for WindowsJob {}
#[cfg(windows)]
#[allow(unsafe_code)]
unsafe impl Sync for WindowsJob {}

#[cfg(windows)]
impl WindowsJob {
    #[allow(unsafe_code)]
    fn attach(child: &Child) -> Result<Self> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(worker_error(std::io::Error::last_os_error()));
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast(),
                u32::try_from(std::mem::size_of_val(&limits)).expect("job limits size fits u32"),
            );
            if configured == 0 || AssignProcessToJobObject(job, child.as_raw_handle().cast()) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(worker_error(error));
            }
            Ok(Self(job))
        }
    }

    #[allow(unsafe_code)]
    fn terminate(&self) {
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct WorkerProcess {
    child: Option<Child>,
    process_tree: Arc<ProcessTree>,
    stdin: Option<BufWriter<ChildStdin>>,
    frames: Option<Receiver<std::io::Result<Vec<u8>>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    reader_done: Option<Receiver<()>>,
    #[cfg(unix)]
    parent_lifeline: Option<OwnedFd>,
    owner: SupervisorOwner,
    lifecycle: Receiver<Lifecycle>,
    next_id: u64,
}

impl WorkerProcess {
    fn spawn(command: &WorkerCommand, permit: OwnedSemaphorePermit, lifecycle: Receiver<Lifecycle>) -> Result<Self> {
        command.validate()?;
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
        let child = process.spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => return Err(worker_error(error)),
        };
        #[cfg(unix)]
        drop(lifeline_read);
        #[cfg(windows)]
        let process_tree = match ProcessTree::attach(&child).map(Arc::new) {
            Ok(process_tree) => process_tree,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        #[cfg(not(windows))]
        let process_tree = Arc::new(ProcessTree::attach(&child));
        let Some(stdin) = child.stdin.take() else {
            force_reap(&mut child, &process_tree);
            return Err(worker_error("worker stdin unavailable"));
        };
        let Some(stdout) = child.stdout.take() else {
            force_reap(&mut child, &process_tree);
            return Err(worker_error("worker stdout unavailable"));
        };
        let (frames_tx, frames) = crossbeam_channel::bounded(1);
        let (reader_done_tx, reader_done) = crossbeam_channel::bounded(1);
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
                let _ = reader_done_tx.send(());
            });
        let reader = match reader {
            Ok(reader) => reader,
            Err(error) => {
                force_reap(&mut child, &process_tree);
                return Err(worker_error(error));
            }
        };
        Ok(Self {
            child: Some(child),
            process_tree,
            stdin: Some(BufWriter::new(stdin)),
            frames: Some(frames),
            reader: Some(reader),
            reader_done: Some(reader_done),
            #[cfg(unix)]
            parent_lifeline: Some(lifeline_write),
            owner,
            lifecycle,
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
        let child = self.child.as_mut().expect("worker child is present");
        let deadline = Instant::now() + REAP_GRACE;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.process_tree.terminate(child);
                    if status.success() {
                        return Ok(());
                    }
                    return Err(worker_error(format!("worker exited with {status} after shutdown ack")));
                }
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                Ok(None) => return Err(worker_error("worker did not exit after shutdown acknowledgement")),
                Err(error) => return Err(worker_error(error)),
            }
        }
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

    fn cleanup(&mut self, force: bool) {
        self.stdin.take();
        #[cfg(unix)]
        self.parent_lifeline.take();
        if let Some(child) = self.child.as_mut()
            && (force || child.try_wait().ok().flatten().is_none())
        {
            force_reap(child, &self.process_tree);
        }
        self.child.take();
        self.frames.take();
        let reader_finished = self
            .reader_done
            .take()
            .is_none_or(|done| done.recv_timeout(READER_SHUTDOWN_GRACE).is_ok());
        if let Some(reader) = self.reader.take() {
            if reader_finished {
                let _ = reader.join();
            } else {
                tracing::warn!("worker stdout reader did not stop before cleanup deadline");
            }
        }
        self.owner.release();
    }
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        self.cleanup(true);
    }
}

fn force_reap(child: &mut Child, process_tree: &ProcessTree) {
    process_tree.terminate(child);
    let _ = child.kill();
    let _ = child.wait();
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
    tree_slot: ProcessTreeSlot,
    bootstrap_result: ResponseSender<PathBuf>,
) {
    let mut worker = match WorkerProcess::spawn(&command, permit, lifecycle.clone()) {
        Ok(worker) => worker,
        Err(error) => {
            let _ = bootstrap_result.send(Err(error));
            return;
        }
    };
    let _ = tree_slot.set(worker.process_tree.clone());
    if let Err(error) = worker.receive_protocol_info() {
        worker.cleanup(true);
        let _ = bootstrap_result.send(Err(error));
        return;
    }
    if bootstrap_result.send(Ok(worker.config_dir())).is_err() {
        worker.cleanup(true);
        return;
    }

    loop {
        crossbeam_channel::select_biased! {
            recv(lifecycle) -> _ => {
                worker.cleanup(true);
                return;
            },
            recv(commands) -> command => {
                let Ok(command) = command else {
                    worker.cleanup(true);
                    return;
                };
                match command {
                    SupervisorCommand::Initialize { request, reply } => match worker.initialize_session(request) {
                        Ok(()) => {
                            let _ = reply.send(Ok(()));
                        }
                        Err(error) => {
                            worker.cleanup(true);
                            let _ = reply.send(Err(error));
                            return;
                        }
                    },
                    SupervisorCommand::Fetch { request, reply } => match worker.fetch(request) {
                        Ok(page) => {
                            let _ = reply.send(Ok(page));
                        }
                        Err(error) => {
                            let terminal = is_terminal_session_error(&error);
                            if terminal {
                                worker.cleanup(true);
                            }
                            let _ = reply.send(Err(error));
                            if terminal {
                                return;
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
                                if terminal {
                                    worker.cleanup(true);
                                }
                                let _ = reply.send(Err(error));
                                if terminal {
                                    return;
                                }
                            }
                        }
                    }
                    SupervisorCommand::Shutdown { reply } => {
                        let result = worker.graceful_shutdown();
                        let graceful = result.is_ok();
                        worker.cleanup(!graceful);
                        let _ = reply.send(result);
                        return;
                    }
                }
            },
        }
    }
}
