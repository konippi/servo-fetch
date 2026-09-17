//! Servo engine bridge.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};
use std::{fmt, thread};

use anyhow::{Result, anyhow};
use dpi::PhysicalSize;
use image::RgbaImage;
use serde_json::Value;
use servo::{
    ConsoleLogLevel, EventLoopWaker, JSValue, JavaScriptEvaluationError, LoadStatus, NavigationRequest, Preferences,
    RenderingContext, ServoBuilder, SoftwareRenderingContext, UrlRequest, UserContentManager, WebView, WebViewBuilder,
    WebViewDelegate, WebViewId,
};
use tokio::sync::{mpsc, oneshot};
use url::Url;

use crate::cookies::CookieSpec;
use crate::{layout, visibility};

const EXTRACTION_BUDGET: Duration = Duration::from_secs(10);
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(1);

fn page_crashed(reason: &str) -> EngineError {
    EngineError::Other(anyhow!("page crashed: {reason}"))
}

/// Servo's builder default for a webview created without an initial URL.
const SHELL_URL: &str = "about:blank";

pub(crate) fn default_user_agent() -> &'static str {
    static UA: OnceLock<String> = OnceLock::new();
    UA.get_or_init(|| {
        let raw = std::env::var("SERVO_FETCH_USER_AGENT")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("servo-fetch/{}", env!("CARGO_PKG_VERSION")));
        crate::net::sanitize_user_agent(raw)
    })
}

const LAYOUT_JS: &str = include_str!("js/layout.js");
const VISIBILITY_JS: &str = include_str!("js/visibility.js");
const HTML_SNAPSHOT_JS: &str = r#"(document.doctype ? "<!DOCTYPE " + document.doctype.name + ">" : "") + (document.documentElement?.outerHTML ?? "")"#;
const MAX_CONSOLE_MESSAGES: usize = 100;
const MAX_CONSOLE_MESSAGE_LEN: usize = 4096;
const MAX_A11Y_NODES: usize = 100_000;

const NOISE_REMOVAL_CSS: &str = visibility::USER_STYLESHEET;

/// Shared wake signal — `notify_all` signals, `wait_and_take` consumes.
#[derive(Default)]
pub(crate) struct WakeFlag {
    flag: Mutex<bool>,
    cv: Condvar,
}

impl WakeFlag {
    /// Block up to `timeout` for a signal, then clear the flag atomically.
    fn wait_and_take(&self, timeout: Duration) -> bool {
        let mut guard = self.flag.lock().unwrap_or_else(PoisonError::into_inner);
        if !*guard {
            let (next, _) = self
                .cv
                .wait_timeout(guard, timeout)
                .unwrap_or_else(PoisonError::into_inner);
            guard = next;
        }
        std::mem::replace(&mut *guard, false)
    }

    fn signal(&self) {
        *self.flag.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.cv.notify_all();
    }
}

#[derive(Clone)]
struct FlagWaker(Arc<WakeFlag>);

impl EventLoopWaker for FlagWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        self.0.signal();
    }
}

thread_local! {
    /// Wake flag owned by `servo_thread`; exposed for `spin_loop` helpers.
    static WAKE: RefCell<Option<Arc<WakeFlag>>> = const { RefCell::new(None) };
}

/// Block up to `timeout` for the next Servo wake.
pub(crate) fn wait_for_wake(timeout: Duration) {
    WAKE.with(|slot| {
        if let Some(flag) = slot.borrow().as_ref() {
            flag.wait_and_take(timeout);
        } else {
            thread::sleep(timeout);
        }
    });
}

struct WebViewState {
    loaded_at: Cell<Option<Instant>>,
    last_ping: Cell<Instant>,
    crashed: RefCell<Option<String>>,
    deferred_load: RefCell<Option<UrlRequest>>,
    a11y_truncated: Cell<bool>,
    a11y_nodes: RefCell<HashMap<servo::accesskit::NodeId, servo::accesskit::Node>>,
    console_messages: RefCell<Vec<ConsoleMessage>>,
}

impl WebViewState {
    fn new(deferred_load: Option<UrlRequest>) -> Self {
        Self {
            loaded_at: Cell::new(None),
            last_ping: Cell::new(Instant::now()),
            crashed: RefCell::new(None),
            deferred_load: RefCell::new(deferred_load),
            a11y_truncated: Cell::new(false),
            a11y_nodes: RefCell::new(HashMap::new()),
            console_messages: RefCell::new(Vec::new()),
        }
    }

    fn next_ping_at(&self) -> Instant {
        self.last_ping.get() + KEEP_ALIVE_INTERVAL
    }

    fn ping_due(&self, now: Instant) -> bool {
        now >= self.next_ping_at()
    }

    fn record_crash(&self, reason: &str) -> EngineError {
        let mut crashed = self.crashed.borrow_mut();
        page_crashed(crashed.get_or_insert_with(|| reason.to_owned()))
    }

    fn fail_if_stopped(&self, result: &Result<JSValue, JavaScriptEvaluationError>) -> Result<(), EngineError> {
        match result {
            Err(JavaScriptEvaluationError::InternalError) => Err(self.record_crash("script thread stopped responding")),
            _ => Ok(()),
        }
    }

    fn crash_error(&self) -> Option<EngineError> {
        self.crashed.borrow().as_deref().map(page_crashed)
    }

    fn take_a11y(&self) -> Option<HashMap<servo::accesskit::NodeId, servo::accesskit::Node>> {
        let mut nodes = self.a11y_nodes.borrow_mut();
        if nodes.is_empty() {
            return None;
        }
        for node in nodes.values_mut() {
            if node.role() == servo::accesskit::Role::PasswordInput {
                node.clear_value();
            }
        }
        Some(std::mem::take(&mut *nodes))
    }

    fn take_console_messages(&self) -> Vec<ConsoleMessage> {
        std::mem::take(&mut self.console_messages.borrow_mut())
    }
}

struct SharedDelegate {
    states: RefCell<HashMap<WebViewId, Rc<WebViewState>>>,
    policy: crate::net::NetworkPolicy,
}

impl SharedDelegate {
    fn register(&self, id: WebViewId, deferred_load: Option<UrlRequest>) -> Rc<WebViewState> {
        let state = Rc::new(WebViewState::new(deferred_load));
        self.states.borrow_mut().insert(id, state.clone());
        state
    }

    fn remove(&self, id: WebViewId) -> Option<Rc<WebViewState>> {
        self.states.borrow_mut().remove(&id)
    }

    fn with_state<R>(&self, id: WebViewId, f: impl FnOnce(&WebViewState) -> R) -> Option<R> {
        let state = self.states.borrow().get(&id).cloned();
        state.map(|s| f(&s))
    }
}

/// A captured console message from the page.
#[derive(serde::Serialize, Clone)]
pub(crate) struct ConsoleMessage {
    pub level: ConsoleLevel,
    pub message: String,
}

/// Console message level or category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ConsoleLevel {
    Log,
    Debug,
    Info,
    Warn,
    Error,
    Trace,
    Dir,
}

impl fmt::Display for ConsoleLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Log => f.write_str("log"),
            Self::Debug => f.write_str("debug"),
            Self::Info => f.write_str("info"),
            Self::Warn => f.write_str("warn"),
            Self::Error => f.write_str("error"),
            Self::Trace => f.write_str("trace"),
            Self::Dir => f.write_str("dir"),
        }
    }
}

impl From<ConsoleLogLevel> for ConsoleLevel {
    fn from(level: ConsoleLogLevel) -> Self {
        match level {
            ConsoleLogLevel::Log => Self::Log,
            ConsoleLogLevel::Debug => Self::Debug,
            ConsoleLogLevel::Info => Self::Info,
            ConsoleLogLevel::Warn => Self::Warn,
            ConsoleLogLevel::Error => Self::Error,
            ConsoleLogLevel::Trace => Self::Trace,
            ConsoleLogLevel::Dir => Self::Dir,
        }
    }
}

impl WebViewDelegate for SharedDelegate {
    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        if webview.url().is_some_and(|u| u.as_str() == SHELL_URL) {
            if let Some(request) = self
                .with_state(webview.id(), |s| s.deferred_load.borrow_mut().take())
                .flatten()
            {
                webview.load_request(request);
            }
        } else if status == LoadStatus::Complete {
            self.with_state(webview.id(), |s| s.loaded_at.set(Some(Instant::now())));
        }
    }

    fn notify_new_frame_ready(&self, webview: WebView) {
        webview.paint();
    }

    fn notify_crashed(&self, webview: WebView, reason: String, backtrace: Option<String>) {
        tracing::debug!(%reason, backtrace = backtrace.as_deref().unwrap_or(""), "page pipeline crashed");
        self.with_state(webview.id(), |state| {
            state.record_crash(&reason);
        });
    }

    fn request_navigation(&self, _webview: WebView, navigation_request: NavigationRequest) {
        let is_http = matches!(navigation_request.url.scheme(), "http" | "https");
        match navigation_request.url.host_str() {
            Some(host) if is_http && self.policy.is_host_allowed(host) => navigation_request.allow(),
            _ => {
                tracing::warn!(url = %navigation_request.url, "blocked navigation");
                navigation_request.deny();
            }
        }
    }

    fn notify_accessibility_tree_update(&self, webview: WebView, tree_update: servo::accesskit::TreeUpdate) {
        self.with_state(webview.id(), |state| {
            let mut nodes = state.a11y_nodes.borrow_mut();
            for (id, node) in tree_update.nodes {
                if nodes.len() >= MAX_A11Y_NODES && !nodes.contains_key(&id) {
                    if !state.a11y_truncated.get() {
                        state.a11y_truncated.set(true);
                        tracing::warn!(limit = MAX_A11Y_NODES, "accessibility tree truncated");
                    }
                    continue;
                }
                nodes.insert(id, node);
            }
        });
    }

    fn show_console_message(&self, webview: WebView, level: ConsoleLogLevel, message: String) {
        self.with_state(webview.id(), |state| {
            let mut msgs = state.console_messages.borrow_mut();
            if msgs.len() < MAX_CONSOLE_MESSAGES {
                let message = if message.len() > MAX_CONSOLE_MESSAGE_LEN {
                    let mut s = message;
                    s.truncate(crate::sanitize::floor_char_boundary(&s, MAX_CONSOLE_MESSAGE_LEN));
                    s.push_str("… (truncated)");
                    s
                } else {
                    message
                };
                msgs.push(ConsoleMessage {
                    level: level.into(),
                    message,
                });
            }
        });
    }
}

/// Captured output of a single page load.
#[derive(Default)]
pub(crate) struct ServoPage {
    pub html: String,
    pub inner_text: Option<String>,
    pub layout_json: Option<String>,
    pub visibility_json: Option<String>,
    pub screenshot: Option<RgbaImage>,
    pub js_result: Option<String>,
    pub a11y: Option<HashMap<servo::accesskit::NodeId, servo::accesskit::Node>>,
    pub console_messages: Vec<ConsoleMessage>,
    pub url: String,
}

/// Parameters for a [`fetch_page`] call.
pub(crate) struct PageOptions<'a> {
    pub url: &'a str,
    pub timeout_secs: u64,
    /// Extra wait after Servo fires `LoadStatus::Complete`.
    pub settle_ms: u64,
    pub mode: FetchMode,
    pub user_agent: Option<&'a str>,
    pub cookies: &'a [CookieSpec],
    pub headers: &'a http::HeaderMap,
}

/// What to do once the page has loaded. Variants are mutually exclusive.
pub(crate) enum FetchMode {
    Content { include_a11y: bool },
    Screenshot { full_page: bool },
    ExecuteJs { expression: String },
}

/// Engine failure. Callers only branch on `Timeout`; `Other` is opaque.
#[derive(Debug, thiserror::Error)]
pub(crate) enum EngineError {
    #[error("page load timed out after {0}s (try increasing --timeout)")]
    Timeout(u64),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub(crate) enum WaitError {
    PageCrashed(EngineError),
    TimedOut,
}

type Reply<T> = oneshot::Sender<Result<T, EngineError>>;

struct PageRequest {
    url: String,
    timeout_secs: u64,
    settle_ms: u64,
    mode: FetchMode,
    user_agent: Option<String>,
    cookies: Vec<CookieSpec>,
    headers: http::HeaderMap,
    reply: Reply<ServoPage>,
}

struct PendingFetch {
    webview: WebView,
    request: PageRequest,
    deadline: Instant,
    state: Rc<WebViewState>,
    dedicated_ctx: Option<Rc<SoftwareRenderingContext>>,
}

impl PendingFetch {
    fn completes_at(&self) -> Instant {
        self.state.loaded_at.get().map_or(self.deadline, |loaded| {
            (loaded + Duration::from_millis(self.request.settle_ms)).min(self.deadline)
        })
    }

    fn is_done(&self, now: Instant) -> bool {
        self.state.crash_error().is_some() || now >= self.completes_at()
    }
}

/// Dispatch envelope for the process-local Servo engine thread.
enum EngineMsg {
    Initialize {
        user_agent: Option<String>,
        cookie_scope: Option<String>,
        cookies: Vec<CookieSpec>,
        reply: Reply<()>,
    },
    Fetch(PageRequest),
    Cookies {
        url: Url,
        reply: Reply<Vec<CookieSpec>>,
    },
}

type EngineTx = mpsc::Sender<EngineMsg>;
type EngineRx = mpsc::Receiver<EngineMsg>;

#[derive(Clone, Default)]
pub(crate) struct EngineConfig {
    pub(crate) policy: crate::net::NetworkPolicy,
    pub(crate) storage: Option<(PathBuf, bool)>,
}

struct Engine {
    requests: EngineTx,
    wake: Arc<WakeFlag>,
    config: EngineConfig,
}

impl Engine {
    fn request<T>(&self, make: impl FnOnce(Reply<T>) -> EngineMsg) -> Result<T, EngineError> {
        let (reply, receive) = oneshot::channel();
        self.requests.try_send(make(reply)).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => Self::queue_full(),
            mpsc::error::TrySendError::Closed(_) => Self::not_running(),
        })?;
        self.wake.signal();
        receive.blocking_recv().unwrap_or_else(|_| Err(Self::stopped()))
    }

    async fn request_async<T>(&self, make: impl FnOnce(Reply<T>) -> EngineMsg) -> Result<T, EngineError> {
        let (reply, receive) = oneshot::channel();
        self.requests.send(make(reply)).await.map_err(|_| Self::not_running())?;
        self.wake.signal();
        receive.await.unwrap_or_else(|_| Err(Self::stopped()))
    }

    fn queue_full() -> EngineError {
        anyhow!("Servo engine queue is full ({PENDING_CAPACITY} pending); back off and retry").into()
    }

    fn not_running() -> EngineError {
        anyhow!("Servo engine is not running (it may have crashed on a previous request)").into()
    }

    fn stopped() -> EngineError {
        anyhow!("Servo engine stopped while processing request").into()
    }
}

/// Servo engine — lives for the process lifetime.
static ENGINE: OnceLock<Engine> = OnceLock::new();
static ENGINE_CONFIG: OnceLock<EngineConfig> = OnceLock::new();

pub(crate) fn configure(config: EngineConfig) -> Result<(), EngineError> {
    if ENGINE.get().is_some() {
        return Err(anyhow!("Servo engine cannot be configured after initialization").into());
    }
    ENGINE_CONFIG
        .set(config)
        .map_err(|_| anyhow!("Servo engine is already configured").into())
}

pub(crate) fn engine_policy() -> crate::net::NetworkPolicy {
    ENGINE
        .get()
        .map(|engine| engine.config.policy)
        .or_else(|| ENGINE_CONFIG.get().map(|config| config.policy))
        .unwrap_or_default()
}

/// Page fetching abstraction for testability.
pub(crate) trait PageFetcher: Send + Sync + 'static {
    fn fetch_page(&self, opts: PageOptions<'_>) -> Result<ServoPage, EngineError>;
}

/// Production implementation backed by the Servo engine.
#[derive(Clone)]
pub(crate) struct ServoFetcher;

impl PageFetcher for ServoFetcher {
    fn fetch_page(&self, opts: PageOptions<'_>) -> Result<ServoPage, EngineError> {
        fetch_page(opts)
    }
}

const PENDING_CAPACITY: usize = 64;

fn ensure_engine() -> &'static Engine {
    ENGINE.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<EngineMsg>(PENDING_CAPACITY);
        let wake = Arc::new(WakeFlag::default());
        let wake_for_thread = wake.clone();
        let config = ENGINE_CONFIG.get().cloned().unwrap_or_default();
        let thread_config = config.clone();
        thread::Builder::new()
            .name("servo-engine".into())
            .spawn(move || servo_thread(rx, wake_for_thread, thread_config))
            .expect("failed to spawn servo thread");
        Engine {
            requests: tx,
            wake,
            config,
        }
    })
}

/// The document URL to expose: the WebView's current URL (falling back to the request) with credentials stripped.
fn document_url(current: Option<&Url>, requested: &str) -> Result<String, EngineError> {
    let candidate = current.map_or(requested, Url::as_str);
    crate::net::validate_url_with_policy(candidate, engine_policy())
        .map(|url| url.to_string())
        .map_err(|error| anyhow!("invalid document URL: {error:?}").into())
}

fn extraction_deadline_for(page_deadline: Instant) -> Instant {
    page_deadline.max(Instant::now() + EXTRACTION_BUDGET)
}

pub(crate) fn initialize_session(
    user_agent: Option<&str>,
    cookie_scope: Option<&str>,
    cookies: &[CookieSpec],
) -> Result<(), EngineError> {
    ensure_engine().request(|reply| EngineMsg::Initialize {
        user_agent: user_agent.map(String::from),
        cookie_scope: cookie_scope.map(String::from),
        cookies: cookies.to_vec(),
        reply,
    })
}

pub(crate) fn fetch_page(opts: PageOptions<'_>) -> Result<ServoPage, EngineError> {
    ensure_engine().request(|reply| {
        EngineMsg::Fetch(PageRequest {
            url: opts.url.to_string(),
            timeout_secs: opts.timeout_secs,
            settle_ms: opts.settle_ms,
            mode: opts.mode,
            user_agent: opts.user_agent.map(String::from),
            cookies: opts.cookies.to_vec(),
            headers: opts.headers.clone(),
            reply,
        })
    })
}

pub(crate) fn cookies_for(url: Url) -> Result<Vec<CookieSpec>, EngineError> {
    ensure_engine().request(|reply| EngineMsg::Cookies { url, reply })
}

pub(crate) async fn fetch_page_async(opts: PageOptions<'_>) -> Result<ServoPage, EngineError> {
    ensure_engine()
        .request_async(|reply| {
            EngineMsg::Fetch(PageRequest {
                url: opts.url.to_string(),
                timeout_secs: opts.timeout_secs,
                settle_ms: opts.settle_ms,
                mode: opts.mode,
                user_agent: opts.user_agent.map(String::from),
                cookies: opts.cookies.to_vec(),
                headers: opts.headers.clone(),
                reply,
            })
        })
        .await
}

fn is_apple_gl_driver_noise(line: &str) -> bool {
    line.contains("GLD_TEXTURE_INDEX_2D is unloadable and bound to sampler type")
}

fn pong_callback(state: &Rc<WebViewState>) -> impl FnOnce(Result<JSValue, JavaScriptEvaluationError>) + 'static {
    let state = Rc::downgrade(state);
    move |result| {
        if let Some(state) = state.upgrade() {
            let _ = state.fail_if_stopped(&result);
        }
    }
}

fn ping_if_due(webview: &WebView, state: &Rc<WebViewState>, now: Instant) {
    if state.ping_due(now) {
        state.last_ping.set(now);
        webview.evaluate_javascript("0", pong_callback(state));
    }
}

/// A loaded page and the deadline every extraction step shares.
pub(crate) struct PageHandle<'a> {
    servo: &'a servo::Servo,
    webview: &'a WebView,
    state: &'a Rc<WebViewState>,
    deadline: Instant,
}

impl PageHandle<'_> {
    pub(crate) fn spin_until<T>(&self, mut ready: impl FnMut() -> Option<T>) -> Result<T, WaitError> {
        loop {
            self.servo.spin_event_loop();
            if let Some(error) = self.state.crash_error() {
                return Err(WaitError::PageCrashed(error));
            }
            if let Some(value) = ready() {
                return Ok(value);
            }
            let now = Instant::now();
            if now >= self.deadline {
                return Err(WaitError::TimedOut);
            }
            // Pinging before load can hit a pipeline the constellation has not activated yet.
            ping_if_due(self.webview, self.state, now);
            wait_for_wake(
                self.deadline
                    .min(self.state.next_ping_at())
                    .saturating_duration_since(now),
            );
        }
    }

    pub(crate) fn eval(&self, script: &str) -> Result<String, EngineError> {
        if let Some(error) = self.state.crash_error() {
            return Err(error);
        }
        if Instant::now() >= self.deadline {
            return Err(eval_error(WaitError::TimedOut));
        }
        let result = Rc::new(Cell::new(None));
        let callback_result = result.clone();
        self.webview
            .evaluate_javascript(script, move |value| callback_result.set(Some(value)));
        let value = self.spin_until(|| result.take()).map_err(eval_error)?;
        self.state.fail_if_stopped(&value)?;
        js_value_to_string(value)
    }

    /// Wait for `document.readyState` to reach `"complete"`.
    ///
    /// TODO(upstream): Servo's `LoadStatus::Complete` fires before the DOM is
    /// fully parsed on pages with heavy inline scripts (e.g. amazon.co.jp); see
    /// servo/servo#41972.
    fn eval_optional(&self, script: &str) -> Result<Option<String>, EngineError> {
        match self.eval(script) {
            Ok(value) => Ok(Some(value)),
            Err(error) => self.state.crash_error().map_or(Ok(None), |_| Err(error)),
        }
    }

    fn wait_for_ready_state(&self) -> Result<(), EngineError> {
        match self
            .spin_until(|| matches!(self.eval("document.readyState"), Ok(value) if value == "complete").then_some(()))
        {
            Ok(()) => Ok(()),
            Err(WaitError::PageCrashed(error)) => Err(error),
            Err(WaitError::TimedOut) => {
                tracing::warn!("document did not finish loading; content may be incomplete");
                Ok(())
            }
        }
    }

    pub(crate) fn webview(&self) -> &WebView {
        self.webview
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the thread owns its receiver for its lifetime"
)]
fn servo_thread(mut request_rx: EngineRx, wake: Arc<WakeFlag>, config: EngineConfig) {
    let _filter = crate::sys::StderrFilter::install(is_apple_gl_driver_noise).ok();

    let (rendering_context, servo) = match build_servo(FlagWaker(wake.clone()), &config) {
        Ok(pair) => pair,
        Err(error) => {
            if let Some(msg) = request_rx.blocking_recv() {
                let error = EngineError::from(error.context("Servo initialization failed"));
                match msg {
                    EngineMsg::Initialize { reply, .. } => {
                        let _ = reply.send(Err(error));
                    }
                    EngineMsg::Fetch(req) => {
                        let _ = req.reply.send(Err(error));
                    }
                    EngineMsg::Cookies { reply, .. } => {
                        let _ = reply.send(Ok(Vec::new()));
                    }
                }
            }
            return;
        }
    };

    WAKE.with(|slot| *slot.borrow_mut() = Some(wake));

    let delegate = Rc::new(SharedDelegate {
        states: RefCell::new(HashMap::new()),
        policy: config.policy,
    });
    let user_content = Rc::new(UserContentManager::new(&servo));
    user_content.add_stylesheet(Rc::new(create_noise_removal_stylesheet()));

    EngineLoop {
        servo,
        rendering_context,
        delegate,
        user_content,
        pending: HashMap::new(),
        baseline_user_agent: default_user_agent().to_owned(),
    }
    .run(request_rx);
}

struct EngineLoop {
    servo: servo::Servo,
    rendering_context: Rc<SoftwareRenderingContext>,
    delegate: Rc<SharedDelegate>,
    user_content: Rc<UserContentManager>,
    pending: HashMap<WebViewId, PendingFetch>,
    baseline_user_agent: String,
}

impl EngineLoop {
    fn run(mut self, mut request_rx: EngineRx) {
        loop {
            while let Ok(msg) = request_rx.try_recv() {
                self.accept(msg);
            }

            if self.pending.is_empty() {
                match request_rx.blocking_recv() {
                    Some(msg) => self.accept(msg),
                    None => return,
                }
                continue;
            }

            self.servo.spin_event_loop();
            self.harvest();

            if let Some(next_wake) = self.pending.values().map(PendingFetch::completes_at).min() {
                wait_for_wake(next_wake.saturating_duration_since(Instant::now()));
            }
        }
    }

    fn accept(&mut self, msg: EngineMsg) {
        match msg {
            EngineMsg::Initialize {
                user_agent,
                cookie_scope,
                cookies,
                reply,
            } => {
                self.baseline_user_agent = user_agent.unwrap_or_else(|| default_user_agent().to_owned());
                self.servo
                    .set_preference("user_agent", servo::PrefValue::Str(self.baseline_user_agent.clone()));
                let result = if cookies.is_empty() {
                    Ok(())
                } else if let Some(scope) = cookie_scope {
                    match Url::parse(&scope) {
                        Ok(scope) => {
                            crate::cookies::seed(&self.servo, &scope, &cookies);
                            Ok(())
                        }
                        Err(error) => Err(anyhow!("invalid cookie scope URL: {error}").into()),
                    }
                } else {
                    Err(anyhow!("cookie_scope is required when session cookies are configured").into())
                };
                let _ = reply.send(result);
            }
            EngineMsg::Fetch(req) => {
                if let Some(pending) = self.start_fetch(req) {
                    self.pending.insert(pending.webview.id(), pending);
                }
            }
            EngineMsg::Cookies { url, reply } => {
                let _ = reply.send(Ok(crate::cookies::capture(&self.servo, &[url])));
            }
        }
    }

    fn start_fetch(&mut self, req: PageRequest) -> Option<PendingFetch> {
        let parsed_url = match Url::parse(&req.url) {
            Ok(url) => url,
            Err(error) => {
                let _ = req.reply.send(Err(anyhow!("bad url: {error}").into()));
                return None;
            }
        };

        let user_agent = resolved_user_agent(req.user_agent.as_deref(), &self.baseline_user_agent);
        self.servo
            .set_preference("user_agent", servo::PrefValue::Str(user_agent.to_owned()));

        crate::cookies::seed(&self.servo, &parsed_url, &req.cookies);

        let dedicated_ctx = if matches!(req.mode, FetchMode::Screenshot { .. }) {
            let size = PhysicalSize::new(layout::VIEWPORT_WIDTH, layout::VIEWPORT_HEIGHT);
            match SoftwareRenderingContext::new(size) {
                Ok(ctx) => {
                    if let Err(error) = ctx.make_current() {
                        let _ = req.reply.send(Err(
                            anyhow!("failed to make screenshot context current: {error:?}").into()
                        ));
                        return None;
                    }
                    Some(Rc::new(ctx))
                }
                Err(error) => {
                    let _ = req
                        .reply
                        .send(Err(anyhow!("failed to create screenshot context: {error:?}").into()));
                    return None;
                }
            }
        } else {
            None
        };

        let rendering_context: Rc<dyn RenderingContext> = match dedicated_ctx.as_ref() {
            Some(ctx) => ctx.clone(),
            None => self.rendering_context.clone(),
        };

        let delegate: Rc<dyn WebViewDelegate> = self.delegate.clone();
        let builder = WebViewBuilder::new(&self.servo, rendering_context)
            .delegate(delegate)
            .user_content_manager(self.user_content.clone());
        let (webview, deferred) = if req.headers.is_empty() {
            (builder.url(parsed_url).build(), None)
        } else {
            (
                builder.build(),
                Some(UrlRequest::new(parsed_url).headers(req.headers.clone())),
            )
        };

        if matches!(req.mode, FetchMode::Content { include_a11y: true }) {
            webview.set_accessibility_active(true);
        }

        let state = self.delegate.register(webview.id(), deferred);
        let deadline = Instant::now() + Duration::from_secs(req.timeout_secs);
        Some(PendingFetch {
            webview,
            request: req,
            deadline,
            state,
            dedicated_ctx,
        })
    }

    fn harvest(&mut self) {
        let now = Instant::now();
        for pending in self
            .pending
            .extract_if(|_, pending| pending.is_done(now))
            .map(|(_, pending)| pending)
            .collect::<Vec<_>>()
        {
            let result = match pending.state.crash_error() {
                Some(error) => Err(error),
                None if pending.state.loaded_at.get().is_none() => {
                    Err(EngineError::Timeout(pending.request.timeout_secs))
                }
                None => self.extract(&pending),
            };
            self.delegate.remove(pending.webview.id());
            drop(pending.webview);
            let _ = pending.request.reply.send(result);
        }
    }

    fn extract(&self, pending: &PendingFetch) -> Result<ServoPage, EngineError> {
        if let Some(ctx) = &pending.dedicated_ctx {
            let _ = ctx.make_current();
        }
        let page = PageHandle {
            servo: &self.servo,
            webview: &pending.webview,
            state: &pending.state,
            deadline: extraction_deadline_for(pending.deadline),
        };
        page.wait_for_ready_state()?;

        let inner_text = page.eval_optional("document.body.innerText")?;
        let layout_json = page.eval_optional(LAYOUT_JS)?;
        let visibility_json = page.eval_optional(VISIBILITY_JS)?;

        // visibility.js stamps data-vf-id on the DOM; the snapshot must include those stamps.
        let html = page.eval(HTML_SNAPSHOT_JS)?;
        let (screenshot, js_result) = match &pending.request.mode {
            FetchMode::Screenshot { full_page } => (crate::screenshot::capture(&page, *full_page)?, None),
            FetchMode::ExecuteJs { expression } => (None, Some(page.eval(expression)?)),
            FetchMode::Content { .. } => (None, None),
        };

        Ok(ServoPage {
            html,
            inner_text,
            layout_json,
            visibility_json,
            screenshot,
            js_result,
            a11y: pending.state.take_a11y(),
            console_messages: pending.state.take_console_messages(),
            url: document_url(pending.webview.url().as_ref(), &pending.request.url)?,
        })
    }
}

fn resolved_user_agent<'a>(request: Option<&'a str>, baseline: &'a str) -> &'a str {
    request.unwrap_or(baseline)
}

fn build_servo(waker: FlagWaker, config: &EngineConfig) -> Result<(Rc<SoftwareRenderingContext>, servo::Servo)> {
    let size = PhysicalSize::new(layout::VIEWPORT_WIDTH, layout::VIEWPORT_HEIGHT);
    let ctx = {
        let ctx =
            SoftwareRenderingContext::new(size).map_err(|e| anyhow!("failed to create rendering context: {e:?}"))?;
        ctx.make_current()
            .map_err(|e| anyhow!("failed to make context current: {e:?}"))?;
        ctx
    };

    let prefs = Preferences {
        accessibility_enabled: true,
        dom_webgpu_enabled: false,
        dom_webxr_enabled: false,
        dom_serviceworker_enabled: false,
        dom_bluetooth_enabled: false,
        dom_intersection_observer_enabled: true,
        dom_indexeddb_enabled: true,
        layout_grid_enabled: true,
        user_agent: default_user_agent().to_owned(),
        ..Preferences::default()
    };

    let (config_dir, temporary_storage) = config
        .storage
        .clone()
        .map_or((None, false), |(path, temporary)| (Some(path), temporary));
    let opts = servo::Opts {
        config_dir,
        temporary_storage,
        ..servo::Opts::default()
    };
    let rc = Rc::new(ctx);
    let servo = ServoBuilder::default()
        .opts(opts)
        .preferences(prefs)
        .event_loop_waker(Box::new(waker))
        .build();
    Ok((rc, servo))
}

fn create_noise_removal_stylesheet() -> servo::user_contents::UserStyleSheet {
    let url = Url::parse("servo-fetch://user-stylesheet/noise-removal").expect("static URL is well-formed");
    servo::user_contents::UserStyleSheet::new(NOISE_REMOVAL_CSS.to_string(), url)
}

fn eval_error(error: WaitError) -> EngineError {
    match error {
        WaitError::PageCrashed(error) => error,
        WaitError::TimedOut => anyhow!("timeout waiting for JS evaluation").into(),
    }
}

fn js_value_to_string(value: Result<JSValue, JavaScriptEvaluationError>) -> Result<String, EngineError> {
    match value {
        Ok(JSValue::String(s)) => Ok(s),
        Ok(JSValue::Undefined | JSValue::Null) => Ok(String::new()),
        Ok(JSValue::Boolean(b)) => Ok(b.to_string()),
        Ok(JSValue::Number(n)) => Ok(n.to_string()),
        Ok(other) => jsvalue_to_json(&other)
            .and_then(|value| serde_json::to_string(&value).map_err(|error| anyhow!("{error}")))
            .map_err(EngineError::from),
        Err(error) => Err(anyhow!("JS eval error: {error:?}").into()),
    }
}

fn jsvalue_to_json(val: &JSValue) -> Result<Value> {
    const MAX_DEPTH: u8 = 64;
    fn convert(val: &JSValue, depth: u8) -> Result<Value> {
        if depth >= MAX_DEPTH {
            return Err(anyhow!("JS value nested too deeply (>{MAX_DEPTH} levels)"));
        }
        Ok(match val {
            JSValue::Undefined | JSValue::Null => Value::Null,
            JSValue::Boolean(b) => Value::Bool(*b),
            JSValue::Number(n) => serde_json::json!(n),
            JSValue::String(s)
            | JSValue::Element(s)
            | JSValue::ShadowRoot(s)
            | JSValue::Frame(s)
            | JSValue::Window(s) => Value::String(s.clone()),
            JSValue::Array(arr) => {
                let items: Result<Vec<_>> = arr.iter().map(|v| convert(v, depth + 1)).collect();
                Value::Array(items?)
            }
            JSValue::Object(map) => {
                let entries: Result<serde_json::Map<_, _>> = map
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), convert(v, depth + 1)?)))
                    .collect();
                Value::Object(entries?)
            }
        })
    }
    convert(val, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_level_display_and_serialization() {
        let cases = [
            (ConsoleLevel::Log, "log"),
            (ConsoleLevel::Debug, "debug"),
            (ConsoleLevel::Info, "info"),
            (ConsoleLevel::Warn, "warn"),
            (ConsoleLevel::Error, "error"),
            (ConsoleLevel::Trace, "trace"),
            (ConsoleLevel::Dir, "dir"),
        ];
        for (level, expected) in cases {
            assert_eq!(level.to_string(), expected);
            assert_eq!(serde_json::to_string(&level).unwrap(), format!("\"{expected}\""));
        }
    }

    #[test]
    fn console_message_serializes() {
        let msg = ConsoleMessage {
            level: ConsoleLevel::Error,
            message: "test".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"level\":\"error\""));
        assert!(json.contains("\"message\":\"test\""));
    }

    #[test]
    fn servo_page_default_is_empty() {
        let page = ServoPage::default();
        assert!(page.html.is_empty());
        assert!(page.inner_text.is_none());
        assert!(page.layout_json.is_none());
        assert!(page.visibility_json.is_none());
        assert!(page.screenshot.is_none());
        assert!(page.js_result.is_none());
        assert!(page.a11y.is_none());
        assert!(page.console_messages.is_empty());
    }

    #[test]
    fn ping_due_fires_once_per_interval() {
        let started = Instant::now();
        let state = WebViewState::new(None);
        state.last_ping.set(started);
        assert!(!state.ping_due(started + KEEP_ALIVE_INTERVAL.saturating_sub(Duration::from_millis(1))));
        assert!(state.ping_due(started + KEEP_ALIVE_INTERVAL));

        let overdue = started + KEEP_ALIVE_INTERVAL * 3;
        assert!(state.ping_due(overdue));
        state.last_ping.set(overdue);
        assert!(!state.ping_due(overdue));
        assert!(!state.ping_due(overdue + KEEP_ALIVE_INTERVAL.saturating_sub(Duration::from_millis(1))));
        assert!(state.ping_due(overdue + KEEP_ALIVE_INTERVAL));
    }

    #[test]
    fn only_internal_eval_errors_record_a_stopped_script_thread() {
        let state = WebViewState::new(None);
        for error in [
            JavaScriptEvaluationError::DocumentNotFound,
            JavaScriptEvaluationError::CompilationFailure,
            JavaScriptEvaluationError::EvaluationFailure(None),
            JavaScriptEvaluationError::WebViewNotReady,
        ] {
            assert!(state.fail_if_stopped(&Err(error)).is_ok());
        }
        assert!(state.crash_error().is_none());
        assert!(
            state
                .fail_if_stopped(&Err(JavaScriptEvaluationError::InternalError))
                .is_err()
        );
        assert_eq!(
            state.crash_error().unwrap().to_string(),
            "page crashed: script thread stopped responding"
        );
    }

    #[test]
    fn late_pong_does_not_retain_ended_state() {
        let state = Rc::new(WebViewState::new(None));
        let weak = Rc::downgrade(&state);
        let callback = pong_callback(&state);
        assert_eq!(Rc::strong_count(&state), 1);
        drop(state);
        callback(Err(JavaScriptEvaluationError::InternalError));
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn jsvalue_to_json_primitives() {
        assert_eq!(jsvalue_to_json(&JSValue::Null).unwrap(), Value::Null);
        assert_eq!(jsvalue_to_json(&JSValue::Undefined).unwrap(), Value::Null);
        assert_eq!(
            jsvalue_to_json(&JSValue::Boolean(true)).unwrap(),
            serde_json::json!(true)
        );
        assert_eq!(
            jsvalue_to_json(&JSValue::Number(42.0)).unwrap(),
            serde_json::json!(42.0)
        );
        assert_eq!(
            jsvalue_to_json(&JSValue::String("hello".into())).unwrap(),
            serde_json::json!("hello")
        );
    }

    #[test]
    fn jsvalue_to_json_array() {
        let val = JSValue::Array(vec![JSValue::Number(1.0), JSValue::String("two".into())]);
        let result = jsvalue_to_json(&val).unwrap();
        assert_eq!(result, serde_json::json!([1.0, "two"]));
    }

    #[test]
    fn jsvalue_to_json_nested_depth_limit() {
        let mut val = JSValue::Null;
        for _ in 0..65 {
            val = JSValue::Array(vec![val]);
        }
        assert!(jsvalue_to_json(&val).is_err());
    }

    #[test]
    fn wake_flag_signal_releases_waiter() {
        let wake = Arc::new(WakeFlag::default());
        let w = wake.clone();
        let handle = thread::spawn(move || w.wait_and_take(Duration::from_secs(5)));
        thread::sleep(Duration::from_millis(10));
        wake.signal();
        assert!(handle.join().unwrap(), "waiter should observe the signal");
    }

    #[test]
    fn wake_flag_wait_and_take_clears() {
        let wake = WakeFlag::default();
        wake.signal();
        assert!(wake.wait_and_take(Duration::from_millis(10)));
        assert!(!wake.wait_and_take(Duration::from_millis(10)));
    }

    #[test]
    fn wake_flag_timeout_returns_false() {
        let wake = WakeFlag::default();
        assert!(
            !wake.wait_and_take(Duration::from_millis(1)),
            "should return false on timeout"
        );
    }

    #[test]
    fn console_level_from_servo() {
        let cases = [
            (ConsoleLogLevel::Log, ConsoleLevel::Log),
            (ConsoleLogLevel::Debug, ConsoleLevel::Debug),
            (ConsoleLogLevel::Info, ConsoleLevel::Info),
            (ConsoleLogLevel::Warn, ConsoleLevel::Warn),
            (ConsoleLogLevel::Error, ConsoleLevel::Error),
            (ConsoleLogLevel::Trace, ConsoleLevel::Trace),
            (ConsoleLogLevel::Dir, ConsoleLevel::Dir),
        ];
        for (source, expected) in cases {
            assert_eq!(ConsoleLevel::from(source), expected);
        }
    }

    #[test]
    fn jsvalue_to_json_element_variants() {
        assert_eq!(
            jsvalue_to_json(&JSValue::Element("div".into())).unwrap(),
            serde_json::json!("div")
        );
        assert_eq!(
            jsvalue_to_json(&JSValue::ShadowRoot("sr".into())).unwrap(),
            serde_json::json!("sr")
        );
        assert_eq!(
            jsvalue_to_json(&JSValue::Frame("f".into())).unwrap(),
            serde_json::json!("f")
        );
        assert_eq!(
            jsvalue_to_json(&JSValue::Window("w".into())).unwrap(),
            serde_json::json!("w")
        );
    }

    #[test]
    fn jsvalue_to_json_object() {
        let mut map = HashMap::new();
        map.insert("key".to_string(), JSValue::Number(1.0));
        let val = JSValue::Object(map);
        let result = jsvalue_to_json(&val).unwrap();
        assert_eq!(result, serde_json::json!({"key": 1.0}));
    }

    #[test]
    fn webview_state_default() {
        let state = WebViewState::new(None);
        assert!(state.loaded_at.get().is_none(), "loaded_at should be None");
        assert!(!state.a11y_truncated.get(), "a11y_truncated should be false");
        assert!(state.a11y_nodes.borrow().is_empty(), "a11y_nodes should be empty");
        assert!(
            state.console_messages.borrow().is_empty(),
            "console_messages should be empty"
        );
    }

    #[test]
    fn take_a11y_redacts_password_values_and_drains_state() {
        let state = WebViewState::new(None);
        let mut password = servo::accesskit::Node::new(servo::accesskit::Role::PasswordInput);
        password.set_value("secret");
        let mut text = servo::accesskit::Node::new(servo::accesskit::Role::TextInput);
        text.set_value("visible");
        state.a11y_nodes.borrow_mut().extend([
            (servo::accesskit::NodeId(1), password),
            (servo::accesskit::NodeId(2), text),
        ]);

        let nodes = state.take_a11y().unwrap();
        assert!(nodes[&servo::accesskit::NodeId(1)].value().is_none());
        assert_eq!(nodes[&servo::accesskit::NodeId(2)].value(), Some("visible"));
        assert!(state.take_a11y().is_none());
    }

    #[test]
    fn request_user_agent_prefers_override_and_falls_back_to_session() {
        assert_eq!(resolved_user_agent(Some("Request/1"), "Session/1"), "Request/1");
        assert_eq!(resolved_user_agent(None, "Session/1"), "Session/1");
    }

    #[test]
    fn extraction_deadline_floors_at_budget_when_page_deadline_passed() {
        let result = extraction_deadline_for(Instant::now());
        let remaining = result.saturating_duration_since(Instant::now());
        assert!(
            remaining >= Duration::from_millis(9_500) && remaining <= EXTRACTION_BUDGET,
            "remaining outside expected window: {remaining:?}"
        );
    }

    #[test]
    fn extraction_deadline_uses_page_deadline_when_far_future() {
        let future = Instant::now() + Duration::from_secs(60);
        let result = extraction_deadline_for(future);
        assert_eq!(result, future);
    }

    #[test]
    fn document_url_prefers_current_url_and_strips_credentials() {
        let current = Url::parse("https://user:secret@example.com/final?q=1#frag").unwrap();
        assert_eq!(
            document_url(Some(&current), "https://example.com/start").unwrap(),
            "https://example.com/final?q=1#frag"
        );
        assert_eq!(
            document_url(None, "https://user:secret@example.com/start").unwrap(),
            "https://example.com/start"
        );
    }
}
