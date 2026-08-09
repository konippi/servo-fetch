//! Strong browser-state isolation using one one-use OS process per logical session.

mod supervisor;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use self::supervisor::{
    ForcePort, SupervisorCommand, SupervisorHandle, receive_response, recv_async, recv_optional_async, response_channel,
};
use crate::cookies::{CookieSpec, CookieWire};
use crate::error::{Error, Result};
use crate::fetch::{FetchMode, FetchOptions, Page};
use crate::worker::protocol::InitializeSession;
use crate::worker::wire::{CrawlWire, FetchWire, MAX_WIRE_CRAWL_CONCURRENCY, MAX_WIRE_DURATION};
use crate::worker::worker_error;
use crate::{CrawlOptions, CrawlResult, NetworkPolicy};
const MAX_SESSIONS: usize = 64;
const MAX_QUEUE_CAPACITY: usize = 1024;
const MAX_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3600);

#[cfg(test)]
mod tests;

static DEFAULT_WORKER_COMMAND: OnceLock<WorkerCommand> = OnceLock::new();
static DEFAULT_BROKER_CONFIG: OnceLock<SessionBrokerConfig> = OnceLock::new();
static DEFAULT_BROKER: OnceLock<SessionBroker> = OnceLock::new();
static DEFAULT_BROKER_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Executable and arguments used to start an isolated worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerCommand {
    program: PathBuf,
    args: Vec<OsString>,
}

impl WorkerCommand {
    /// Construct a command that enters [`run_worker_stdio`] before using Servo.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    /// Append one worker argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append worker arguments.
    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    fn unconfigured() -> Self {
        Self::new(PathBuf::new())
    }

    fn validate(&self) -> Result<()> {
        if self.program.as_os_str().is_empty() {
            return Err(invalid_config("worker command is not configured"));
        }
        if !self.program.is_absolute() {
            return Err(invalid_config("worker executable path must be absolute"));
        }
        Ok(())
    }

    fn resolved() -> Result<Self> {
        DEFAULT_WORKER_COMMAND.get().cloned().ok_or_else(|| {
            invalid_config("worker command is not configured; call set_default_worker_command or set worker_command")
        })
    }
}

fn invalid_config(reason: impl Into<String>) -> Error {
    Error::InvalidSessionConfig { reason: reason.into() }
}

/// Set the worker command before default broker initialization; PATH lookup is intentionally disabled.
pub fn set_default_worker_command(command: WorkerCommand) -> Result<()> {
    command.validate()?;
    let _guard = DEFAULT_BROKER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if DEFAULT_BROKER.get().is_some() || DEFAULT_BROKER_CONFIG.get().is_some() {
        return Err(worker_error(
            "default session broker is already configured or initialized",
        ));
    }
    DEFAULT_WORKER_COMMAND
        .set(command)
        .map_err(|_| worker_error("default worker command is already configured"))
}

/// Configure the process-wide broker before its first use.
pub fn configure_default_broker(mut config: SessionBrokerConfig) -> Result<()> {
    if config.worker_command.program.as_os_str().is_empty() {
        config.worker_command = WorkerCommand::resolved()?;
    }
    config.validate()?;
    let _guard = DEFAULT_BROKER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if DEFAULT_BROKER.get().is_some() {
        return Err(worker_error("default session broker is already initialized"));
    }
    DEFAULT_BROKER_CONFIG
        .set(config)
        .map_err(|_| worker_error("default session broker is already configured"))
}

/// Capacity and startup policy for a [`SessionBroker`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SessionBrokerConfig {
    /// Maximum simultaneously-live logical sessions.
    pub(crate) max_sessions: usize,
    /// Maximum number of callers allowed to wait for capacity.
    pub(crate) queue_capacity: usize,
    /// Maximum wait for session capacity.
    pub(crate) acquire_timeout: Duration,
    /// Number of clean, uninitialized workers created eagerly.
    pub(crate) prewarm: usize,
    /// Worker executable configuration.
    pub(crate) worker_command: WorkerCommand,
}

impl SessionBrokerConfig {
    /// Set the maximum number of simultaneously-live sessions.
    #[must_use]
    pub fn max_sessions(mut self, max_sessions: usize) -> Self {
        self.max_sessions = max_sessions;
        self
    }

    /// Set the maximum number of callers waiting for capacity.
    #[must_use]
    pub fn queue_capacity(mut self, queue_capacity: usize) -> Self {
        self.queue_capacity = queue_capacity;
        self
    }

    /// Set the maximum wait for session capacity.
    #[must_use]
    pub fn acquire_timeout(mut self, acquire_timeout: Duration) -> Self {
        self.acquire_timeout = acquire_timeout;
        self
    }

    /// Set the number of clean workers created eagerly.
    #[must_use]
    pub fn prewarm(mut self, prewarm: usize) -> Self {
        self.prewarm = prewarm;
        self
    }

    /// Set the executable used for isolated workers.
    #[must_use]
    pub fn worker_command(mut self, worker_command: WorkerCommand) -> Self {
        self.worker_command = worker_command;
        self
    }

    fn validate(&self) -> Result<()> {
        if self.max_sessions == 0 || self.max_sessions > MAX_SESSIONS {
            return Err(invalid_config("max_sessions must be between 1 and 64"));
        }
        if self.queue_capacity > MAX_QUEUE_CAPACITY {
            return Err(invalid_config("queue_capacity must not exceed 1024"));
        }
        if self.prewarm > self.max_sessions {
            return Err(invalid_config("prewarm must not exceed max_sessions"));
        }
        if self.acquire_timeout.is_zero() || self.acquire_timeout > MAX_ACQUIRE_TIMEOUT {
            return Err(invalid_config(
                "acquire_timeout must be greater than zero and at most 3600s",
            ));
        }
        self.worker_command.validate()
    }
}

impl Default for SessionBrokerConfig {
    fn default() -> Self {
        let cpu_default = std::thread::available_parallelism().map_or(2, usize::from).clamp(1, 4);
        let max_sessions = env_usize("SERVO_FETCH_MAX_WORKERS")
            .unwrap_or(cpu_default)
            .clamp(1, MAX_SESSIONS);
        let queue_capacity = env_usize("SERVO_FETCH_SESSION_QUEUE")
            .unwrap_or_else(|| max_sessions.saturating_mul(2))
            .min(MAX_QUEUE_CAPACITY);
        let prewarm = env_usize("SERVO_FETCH_PREWARM").unwrap_or(0).min(max_sessions);
        let acquire_timeout = Duration::from_secs(
            env_usize("SERVO_FETCH_ACQUIRE_TIMEOUT_SECS")
                .and_then(|value| u64::try_from(value).ok())
                .unwrap_or(30)
                .clamp(1, MAX_ACQUIRE_TIMEOUT.as_secs()),
        );
        Self {
            max_sessions,
            queue_capacity,
            acquire_timeout,
            prewarm,
            worker_command: WorkerCommand::unconfigured(),
        }
    }
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok()?.parse().ok()
}

/// Immutable state applied once when a logical browser session starts.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct BrowserSessionConfig {
    /// Session-wide User-Agent.
    pub(crate) user_agent: Option<String>,
    /// Cookies seeded once before the first navigation.
    pub(crate) cookies: Vec<CookieSpec>,
    /// URL used to validate and scope session cookies.
    pub(crate) cookie_scope: Option<String>,
}

impl BrowserSessionConfig {
    /// Create an empty session configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the immutable session User-Agent.
    #[must_use]
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(crate::net::sanitize_user_agent(user_agent.into()));
        self
    }

    /// Seed cookies once, scoped to `url`.
    #[must_use]
    pub fn cookies(mut self, url: impl Into<String>, cookies: Vec<CookieSpec>) -> Self {
        self.cookie_scope = Some(url.into());
        self.cookies = cookies;
        self
    }
}

/// Cancellation handle for one browser-session acquisition and its session.
#[derive(Debug, Clone, Default)]
pub struct SessionCancellation {
    inner: Arc<CancellationState>,
}

#[derive(Debug, Default)]
struct CancellationState {
    state: Mutex<CancellationStateInner>,
    notified: tokio::sync::Notify,
}

#[derive(Debug, Default)]
struct CancellationStateInner {
    cancelled: bool,
    force_port: Option<ForcePort>,
}

impl SessionCancellation {
    /// Create an active cancellation handle.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancel acquisition and force-close its worker if one has been attached.
    ///
    /// A handle tracks at most one live acquisition; reusing it across
    /// concurrent acquisitions only force-closes the most recently attached one.
    pub fn cancel(&self) {
        let force_port = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.cancelled = true;
            state.force_port.clone()
        };
        if let Some(force_port) = force_port {
            force_port.force();
        }
        self.inner.notified.notify_waiters();
    }

    /// Return whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cancelled
    }

    fn attach(&self, force_port: &ForcePort) -> bool {
        let cancelled = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.force_port = Some(force_port.clone());
            state.cancelled
        };
        if cancelled {
            force_port.force();
        }
        !cancelled
    }

    async fn cancelled(&self) {
        loop {
            let notified = self.inner.notified.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// Bounded owner of clean workers and live logical-session permits.
#[derive(Debug, Clone)]
pub struct SessionBroker {
    inner: Arc<BrokerInner>,
}

#[derive(Debug)]
struct BrokerInner {
    config: SessionBrokerConfig,
    permits: Arc<Semaphore>,
    queue_slots: Arc<Semaphore>,
    prewarmed: Mutex<Vec<SupervisorHandle>>,
}

impl SessionBroker {
    /// Construct a broker and eagerly start only the configured clean workers.
    pub fn new(mut config: SessionBrokerConfig) -> Result<Self> {
        if config.worker_command.program.as_os_str().is_empty() {
            config.worker_command = WorkerCommand::resolved()?;
        }
        config.validate()?;
        let permits = Arc::new(Semaphore::new(config.max_sessions));
        let mut prewarmed = Vec::with_capacity(config.prewarm);
        for _ in 0..config.prewarm {
            let permit = permits
                .clone()
                .try_acquire_owned()
                .map_err(|_| worker_error("failed to reserve prewarm permit"))?;
            prewarmed.push(SupervisorHandle::spawn(config.worker_command.clone(), permit, None)?);
        }
        Ok(Self {
            inner: Arc::new(BrokerInner {
                permits,
                queue_slots: Arc::new(Semaphore::new(config.queue_capacity)),
                prewarmed: Mutex::new(prewarmed),
                config,
            }),
        })
    }

    /// Acquire a fresh isolated browser session.
    pub async fn session(&self, config: BrowserSessionConfig) -> Result<BrowserSession> {
        self.session_with_cancellation(config, &SessionCancellation::new())
            .await
    }

    /// Acquire a session linked to a cancellation handle created before this call.
    pub async fn session_with_cancellation(
        &self,
        config: BrowserSessionConfig,
        cancellation: &SessionCancellation,
    ) -> Result<BrowserSession> {
        validate_session_config(&config)?;
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let clean = self.take_prewarmed();
        let permit = if clean.is_none() {
            Some(self.acquire_permit(cancellation).await?)
        } else {
            None
        };
        let mut acquisition = AcquisitionGuard::new(cancellation.clone());
        let broker = self.clone();
        let cancellation = cancellation.clone();
        let result =
            tokio::task::spawn_blocking(move || broker.start_session_blocking(config, &cancellation, clean, permit))
                .await;
        // The guard only exists to abort detached startup work when this future
        // is dropped mid-flight; once the join completes, ordinary failures must
        // not poison the caller's cancellation handle.
        acquisition.disarm();
        result.map_err(|error| worker_error(format!("worker startup task failed: {error}")))?
    }

    /// Blocking counterpart of [`Self::session`].
    pub fn session_blocking(&self, config: BrowserSessionConfig) -> Result<BrowserSession> {
        self.session_blocking_with_cancellation(config, &SessionCancellation::new())
    }

    /// Blocking counterpart of [`Self::session_with_cancellation`].
    pub fn session_blocking_with_cancellation(
        &self,
        config: BrowserSessionConfig,
        cancellation: &SessionCancellation,
    ) -> Result<BrowserSession> {
        crate::runtime::block_on(self.session_with_cancellation(config, cancellation))
            .map_err(|error| worker_error(error.to_string()))?
    }

    fn take_prewarmed(&self) -> Option<SupervisorHandle> {
        self.inner
            .prewarmed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop()
    }

    async fn acquire_permit(&self, cancellation: &SessionCancellation) -> Result<OwnedSemaphorePermit> {
        if let Ok(permit) = self.inner.permits.clone().try_acquire_owned() {
            return Ok(permit);
        }
        let queue_slot = self
            .inner
            .queue_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::SessionBrokerFull)?;
        let acquire = self.inner.permits.clone().acquire_owned();
        let timeout = self.inner.config.acquire_timeout;
        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(cancelled_error()),
            result = tokio::time::timeout(timeout, acquire) => match result {
                Ok(Ok(permit)) => Ok(permit),
                Ok(Err(_)) => Err(worker_error("session broker closed")),
                Err(_) => Err(Error::SessionAcquireTimeout { timeout }),
            },
        };
        drop(queue_slot);
        result
    }

    fn start_session_blocking(
        &self,
        config: BrowserSessionConfig,
        cancellation: &SessionCancellation,
        clean: Option<SupervisorHandle>,
        permit: Option<OwnedSemaphorePermit>,
    ) -> Result<BrowserSession> {
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let mut supervisor = match clean {
            Some(supervisor) => supervisor,
            None => SupervisorHandle::spawn(
                self.inner.config.worker_command.clone(),
                permit.expect("non-prewarmed startup owns a permit"),
                Some(cancellation),
            )?,
        };
        if !cancellation.attach(&supervisor.force_port) {
            return Err(cancelled_error());
        }
        let user_agent = config.user_agent.clone();
        let initialization = InitializeSession {
            permissive_network: crate::bridge::engine_policy() == NetworkPolicy::PERMISSIVE,
            user_agent: config.user_agent,
            cookies: CookieWire::from_specs(&config.cookies),
            cookie_scope: config.cookie_scope,
            config_dir: supervisor.config_dir.clone(),
            temporary_storage: true,
        };
        supervisor.initialize_session(initialization)?;
        Ok(BrowserSession {
            supervisor: Some(supervisor),
            user_agent,
        })
    }
}

fn validate_session_config(config: &BrowserSessionConfig) -> Result<()> {
    if !config.cookies.is_empty() && config.cookie_scope.is_none() {
        return Err(worker_error(
            "cookie_scope is required when session cookies are configured",
        ));
    }
    if let Some(scope) = config.cookie_scope.as_deref() {
        crate::net::validate_url(scope)?;
    }
    Ok(())
}

struct AcquisitionGuard {
    cancellation: SessionCancellation,
    armed: bool,
}

impl AcquisitionGuard {
    fn new(cancellation: SessionCancellation) -> Self {
        Self {
            cancellation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for AcquisitionGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel();
        }
    }
}

/// An isolated context with session-scoped site data and fresh `WebView`s; configure a compatible [`WorkerCommand`] before construction.
#[derive(Debug)]
pub struct BrowserSession {
    supervisor: Option<SupervisorHandle>,
    user_agent: Option<String>,
}

impl BrowserSession {
    /// Force-close this session without waiting. Further operations fail.
    pub fn cancel(&mut self) {
        if let Some(mut supervisor) = self.supervisor.take() {
            supervisor.force();
        }
    }

    /// Gracefully close this session, force-killing the worker if it does not exit.
    pub async fn close(mut self) -> Result<()> {
        let Some(mut supervisor) = self.supervisor.take() else {
            return Ok(());
        };
        let receive = supervisor.begin_close()?;
        recv_async(receive, "session close").await?;
        supervisor.disarm();
        Ok(())
    }

    /// Blocking counterpart of [`Self::close`].
    pub fn close_blocking(mut self) -> Result<()> {
        let Some(mut supervisor) = self.supervisor.take() else {
            return Ok(());
        };
        let result = supervisor.close_blocking();
        supervisor.disarm();
        result
    }

    /// Force-close this session and wait for supervisor-owned cleanup to finish.
    pub async fn force_close(mut self) -> Result<()> {
        let Some(mut supervisor) = self.supervisor.take() else {
            return Ok(());
        };
        let terminal = supervisor.begin_force()?;
        recv_async(terminal, "forced session close").await?;
        supervisor.disarm();
        Ok(())
    }

    /// Create a session from the process-wide default broker.
    pub async fn new(config: BrowserSessionConfig) -> Result<Self> {
        default_broker()?.session(config).await
    }

    /// Create a session linked to a pre-existing cancellation handle.
    pub async fn new_with_cancellation(
        config: BrowserSessionConfig,
        cancellation: &SessionCancellation,
    ) -> Result<Self> {
        default_broker()?.session_with_cancellation(config, cancellation).await
    }

    /// Create a blocking session from the process-wide default broker.
    pub fn new_blocking(config: BrowserSessionConfig) -> Result<Self> {
        default_broker()?.session_blocking(config)
    }

    /// Create a blocking session linked to a pre-existing cancellation handle.
    pub fn new_blocking_with_cancellation(
        config: BrowserSessionConfig,
        cancellation: &SessionCancellation,
    ) -> Result<Self> {
        default_broker()?.session_blocking_with_cancellation(config, cancellation)
    }

    /// Fetch in this logical session. Per-request UA/cookies are rejected.
    pub async fn fetch(&mut self, opts: &FetchOptions) -> Result<Page> {
        validate_session_fetch(opts)?;
        crate::net::validate_url(&opts.url)?;
        let (reply, receive) = response_channel();
        let command = SupervisorCommand::Fetch {
            request: FetchWire::from_options(opts),
            reply,
        };
        let force_port = self.supervisor_mut()?.force_port.clone();
        self.supervisor_mut()?.send(command)?;
        let mut guard = OperationGuard::new(force_port);
        let response = recv_async(receive, "isolated fetch").await;
        guard.disarm();
        let (wire, screenshot) = self.finish_operation(response)?;
        self.finish_operation(wire.into_page(screenshot))
    }

    /// Blocking fetch in this logical session.
    pub fn fetch_blocking(&mut self, opts: &FetchOptions) -> Result<Page> {
        validate_session_fetch(opts)?;
        crate::net::validate_url(&opts.url)?;
        let (reply, receive) = response_channel();
        self.supervisor_mut()?.send(SupervisorCommand::Fetch {
            request: FetchWire::from_options(opts),
            reply,
        })?;
        let response = receive_response(&receive, "isolated fetch");
        let (wire, screenshot) = self.finish_operation(response)?;
        self.finish_operation(wire.into_page(screenshot))
    }

    /// Crawl in this logical session.
    pub async fn crawl(&mut self, opts: &CrawlOptions) -> Result<Vec<CrawlResult>> {
        let mut results = Vec::new();
        self.crawl_each(opts, |result| results.push(result)).await?;
        Ok(results)
    }

    /// Stream crawl results while preserving async cancellation semantics.
    pub async fn crawl_each<F>(&mut self, opts: &CrawlOptions, mut on_page: F) -> Result<()>
    where
        F: FnMut(CrawlResult),
    {
        validate_session_crawl(opts)?;
        crate::net::validate_url(&opts.url)?;
        let mut request = opts.clone();
        request.robots_user_agent.clone_from(&self.user_agent);
        let (events, receive_events) = crossbeam_channel::bounded(1);
        let (reply, receive) = response_channel();
        let force_port = self.supervisor_mut()?.force_port.clone();
        self.supervisor_mut()?.send(SupervisorCommand::Crawl {
            request: CrawlWire::from_options(&request),
            events,
            reply,
        })?;
        let mut guard = OperationGuard::new(force_port);
        while let Some(result) = recv_optional_async(receive_events.clone()).await? {
            let callback = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_page(result)));
            if let Err(payload) = callback {
                drop(guard);
                std::panic::resume_unwind(payload);
            }
        }
        let response = recv_async(receive, "isolated crawl").await;
        guard.disarm();
        self.finish_operation(response)
    }

    /// Blocking crawl in this logical session.
    pub fn crawl_blocking(&mut self, opts: &CrawlOptions) -> Result<Vec<CrawlResult>> {
        let mut results = Vec::new();
        self.crawl_each_blocking(opts, |result| results.push(result))?;
        Ok(results)
    }

    /// Stream a crawl from this logical session over bounded worker frames.
    pub fn crawl_each_blocking<F>(&mut self, opts: &CrawlOptions, mut on_page: F) -> Result<()>
    where
        F: FnMut(CrawlResult),
    {
        validate_session_crawl(opts)?;
        crate::net::validate_url(&opts.url)?;
        let mut request = opts.clone();
        request.robots_user_agent.clone_from(&self.user_agent);
        let (events, receive_events) = crossbeam_channel::bounded(1);
        let (reply, receive) = response_channel();
        let force_port = self.supervisor_mut()?.force_port.clone();
        self.supervisor_mut()?.send(SupervisorCommand::Crawl {
            request: CrawlWire::from_options(&request),
            events,
            reply,
        })?;
        while let Ok(result) = receive_events.recv() {
            let callback = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_page(result)));
            if let Err(payload) = callback {
                force_port.force();
                std::panic::resume_unwind(payload);
            }
        }
        let response = receive_response(&receive, "isolated crawl");
        self.finish_operation(response)
    }

    fn finish_operation<T>(&mut self, result: Result<T>) -> Result<T> {
        if result.as_ref().is_err_and(is_terminal_session_error)
            && let Some(mut supervisor) = self.supervisor.take()
        {
            supervisor.disarm();
        }
        result
    }

    fn supervisor_mut(&mut self) -> Result<&mut SupervisorHandle> {
        self.supervisor
            .as_mut()
            .ok_or_else(|| worker_error("browser session is closed"))
    }
}

struct OperationGuard {
    force_port: ForcePort,
    armed: bool,
}

impl OperationGuard {
    fn new(force_port: ForcePort) -> Self {
        Self {
            force_port,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if self.armed {
            self.force_port.force();
        }
    }
}

fn default_broker() -> Result<&'static SessionBroker> {
    if let Some(broker) = DEFAULT_BROKER.get() {
        return Ok(broker);
    }
    let _guard = DEFAULT_BROKER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(broker) = DEFAULT_BROKER.get() {
        return Ok(broker);
    }
    let config = match DEFAULT_BROKER_CONFIG.get() {
        Some(config) => config.clone(),
        None => SessionBrokerConfig::default().worker_command(WorkerCommand::resolved()?),
    };
    let broker = SessionBroker::new(config)?;
    DEFAULT_BROKER
        .set(broker)
        .map_err(|_| worker_error("default session broker initialized concurrently"))?;
    DEFAULT_BROKER
        .get()
        .ok_or_else(|| worker_error("default session broker initialization failed"))
}

fn validate_session_fetch(opts: &FetchOptions) -> Result<()> {
    validate_immutable_settings(opts.user_agent.as_ref(), &opts.cookies, &opts.headers)?;
    validate_operation_duration("fetch timeout", opts.effective_timeout())?;
    validate_operation_duration("fetch settle", opts.effective_settle())?;
    if matches!(opts.mode, FetchMode::Content { .. }) && crate::pdf::looks_like_pdf_url(&opts.url) {
        return Err(Error::UnsupportedSessionOperation {
            operation: "PDF extraction",
            reason: "session-aware PDF networking is unavailable; use the one-shot fetch API",
        });
    }
    Ok(())
}

fn validate_session_crawl(opts: &CrawlOptions) -> Result<()> {
    validate_immutable_settings(opts.user_agent.as_ref(), &opts.cookies, &opts.headers)?;
    validate_operation_duration("crawl timeout", opts.timeout)?;
    validate_operation_duration("crawl settle", opts.settle)?;
    if let Some(delay) = opts.delay {
        validate_operation_duration("crawl delay", delay)?;
    }
    if !(1..=MAX_WIRE_CRAWL_CONCURRENCY).contains(&opts.concurrency) {
        return Err(worker_error(format!(
            "crawl concurrency must be between 1 and {MAX_WIRE_CRAWL_CONCURRENCY}"
        )));
    }
    opts.validate()
}

fn validate_operation_duration(field: &str, duration: Duration) -> Result<()> {
    if duration > MAX_WIRE_DURATION {
        return Err(worker_error(format!(
            "{field} exceeds the maximum of {} seconds",
            MAX_WIRE_DURATION.as_secs()
        )));
    }
    Ok(())
}

fn validate_immutable_settings(
    user_agent: Option<&String>,
    cookies: &[CookieSpec],
    headers: &http::HeaderMap,
) -> Result<()> {
    if user_agent.is_some()
        || !cookies.is_empty()
        || headers.contains_key(http::header::USER_AGENT)
        || headers.contains_key(http::header::COOKIE)
    {
        return Err(worker_error(
            "User-Agent and cookies are immutable session settings; use BrowserSessionConfig",
        ));
    }
    crate::headers::validate_map(headers)
}

fn cancelled_error() -> Error {
    Error::SessionCancelled
}

fn is_terminal_session_error(error: &Error) -> bool {
    matches!(
        error,
        Error::SessionCancelled | Error::WorkerProtocolTimeout { .. } | Error::WorkerUnavailable { .. }
    )
}
