//! Servo engine bridge.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use dpi::PhysicalSize;
use image::RgbaImage;
use servo::{
    ConsoleLogLevel, EventLoopWaker, JSValue, LoadStatus, NavigationRequest, Preferences, RenderingContext,
    ServoBuilder, SoftwareRenderingContext, UserContentManager, WebView, WebViewBuilder, WebViewDelegate, WebViewId,
};

use crate::layout;

const JS_EVAL_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn default_user_agent() -> &'static str {
    static UA: OnceLock<String> = OnceLock::new();
    UA.get_or_init(|| {
        let raw = std::env::var("SERVO_FETCH_USER_AGENT")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("servo-fetch/{}", env!("CARGO_PKG_VERSION")));
        crate::engine::sanitize_user_agent(raw)
    })
}
/// Max wait before we re-check time-based conditions.
pub(crate) const FALLBACK_WAIT: Duration = Duration::from_millis(5);
const LAYOUT_JS: &str = include_str!("js/layout.js");
const MAX_CONSOLE_MESSAGES: usize = 100;
const MAX_CONSOLE_MESSAGE_LEN: usize = 4096;
const MAX_A11Y_NODES: usize = 100_000;

const NOISE_REMOVAL_CSS: &str = "\
[aria-label*=\"cookie\" i], [aria-label*=\"consent\" i], \
[class*=\"cookie-banner\" i], [class*=\"cookie-consent\" i], \
[id*=\"cookie\" i][class*=\"banner\" i], \
[class*=\"newsletter-popup\" i], [class*=\"subscribe-modal\" i] \
{ display: none !important; }";

/// Shared wake signal — `notify_all` signals, `wait_and_take` consumes.
#[derive(Default)]
pub(crate) struct WakeFlag {
    flag: Mutex<bool>,
    cv: Condvar,
}

impl WakeFlag {
    /// Block up to `timeout` for a signal, then clear the flag atomically.
    fn wait_and_take(&self, timeout: Duration) -> bool {
        let mut guard = self.flag.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !*guard {
            let (next, _) = self
                .cv
                .wait_timeout(guard, timeout)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard = next;
        }
        std::mem::replace(&mut *guard, false)
    }

    fn signal(&self) {
        *self.flag.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
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
            std::thread::sleep(timeout);
        }
    });
}

#[derive(Default)]
struct WebViewState {
    loaded_at: Cell<Option<Instant>>,
    a11y_truncated: Cell<bool>,
    a11y_nodes: RefCell<HashMap<servo::accesskit::NodeId, servo::accesskit::Node>>,
    console_messages: RefCell<Vec<ConsoleMessage>>,
}

struct SharedDelegate {
    states: RefCell<HashMap<WebViewId, Rc<WebViewState>>>,
    policy: crate::net::NetworkPolicy,
}

impl SharedDelegate {
    fn register(&self, id: WebViewId) -> Rc<WebViewState> {
        let state = Rc::new(WebViewState::default());
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

/// Console message severity level.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ConsoleLevel {
    Log,
    Debug,
    Info,
    Warn,
    Error,
    Trace,
}

impl std::fmt::Display for ConsoleLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Log => f.write_str("log"),
            Self::Debug => f.write_str("debug"),
            Self::Info => f.write_str("info"),
            Self::Warn => f.write_str("warn"),
            Self::Error => f.write_str("error"),
            Self::Trace => f.write_str("trace"),
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
        }
    }
}

impl WebViewDelegate for SharedDelegate {
    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        if status == LoadStatus::Complete {
            self.with_state(webview.id(), |s| {
                s.loaded_at.set(Some(Instant::now()));
            });
        }
    }

    fn notify_new_frame_ready(&self, webview: WebView) {
        webview.paint();
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
    pub screenshot: Option<RgbaImage>,
    pub js_result: Option<String>,
    pub accessibility_tree: Option<String>,
    pub console_messages: Vec<ConsoleMessage>,
}

/// Parameters for a [`fetch_page`] call.
pub(crate) struct FetchOptions<'a> {
    pub url: &'a str,
    pub timeout_secs: u64,
    /// Extra wait after Servo fires `LoadStatus::Complete`.
    pub settle_ms: u64,
    pub mode: FetchMode,
    pub user_agent: Option<&'a str>,
}

/// What to do once the page has loaded. Variants are mutually exclusive.
pub(crate) enum FetchMode {
    Content { include_a11y: bool },
    Screenshot { full_page: bool },
    ExecuteJs { expression: String },
}

struct FetchRequest {
    url: String,
    timeout_secs: u64,
    settle_ms: u64,
    mode: FetchMode,
    user_agent: Option<String>,
    reply: mpsc::Sender<Result<ServoPage>>,
}

struct PendingFetch {
    webview: WebView,
    request: FetchRequest,
    deadline: Instant,
    state: Rc<WebViewState>,
    dedicated_ctx: Option<Rc<SoftwareRenderingContext>>,
}

struct Engine {
    requests: mpsc::SyncSender<FetchRequest>,
    wake: Arc<WakeFlag>,
    policy: crate::net::NetworkPolicy,
}

/// Servo engine — lives for the process lifetime. Shutdown is via process exit.
static ENGINE: OnceLock<Engine> = OnceLock::new();

pub(crate) fn engine_policy() -> crate::net::NetworkPolicy {
    ENGINE.get().map_or(crate::net::NetworkPolicy::STRICT, |e| e.policy)
}

/// Page fetching abstraction for testability.
pub(crate) trait PageFetcher: Send + Sync + 'static {
    fn fetch_page(&self, opts: FetchOptions<'_>) -> Result<ServoPage>;
}

/// Production implementation backed by the Servo engine.
#[derive(Clone)]
pub(crate) struct ServoFetcher;

impl PageFetcher for ServoFetcher {
    fn fetch_page(&self, opts: FetchOptions<'_>) -> Result<ServoPage> {
        fetch_page(opts)
    }
}

pub(crate) fn fetch_page(opts: FetchOptions<'_>) -> Result<ServoPage> {
    /// Max outstanding requests queued toward the engine.
    const PENDING_CAPACITY: usize = 64;

    let engine = ENGINE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<FetchRequest>(PENDING_CAPACITY);
        let wake = Arc::new(WakeFlag::default());
        let wake_for_thread = wake.clone();
        let policy = crate::net::NetworkPolicy::STRICT;
        std::thread::Builder::new()
            .name("servo-engine".into())
            .spawn(move || servo_thread(rx, wake_for_thread, policy))
            .expect("failed to spawn servo thread");
        Engine {
            requests: tx,
            wake,
            policy,
        }
    });

    let (reply_tx, reply_rx) = mpsc::channel();
    let deadline =
        Duration::from_secs(opts.timeout_secs) + Duration::from_millis(opts.settle_ms) + Duration::from_secs(2);
    engine
        .requests
        .send(FetchRequest {
            url: opts.url.to_string(),
            timeout_secs: opts.timeout_secs,
            settle_ms: opts.settle_ms,
            mode: opts.mode,
            user_agent: opts.user_agent.map(String::from),
            reply: reply_tx,
        })
        .map_err(|_| anyhow!("Servo engine is not running (it may have crashed on a previous request)"))?;
    // Nudge the engine so it checks the request queue even if it was idle.
    engine.wake.signal();

    match reply_rx.recv_timeout(deadline) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(anyhow!("Servo engine did not respond within {}s", deadline.as_secs()))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!("Servo engine crashed while processing this page")),
    }
}

fn is_apple_gl_driver_noise(line: &str) -> bool {
    line.contains("GLD_TEXTURE_INDEX_2D is unloadable and bound to sampler type")
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the thread owns its receiver for its lifetime"
)]
fn servo_thread(request_rx: mpsc::Receiver<FetchRequest>, wake: Arc<WakeFlag>, policy: crate::net::NetworkPolicy) {
    let _filter = crate::sys::StderrFilter::install(is_apple_gl_driver_noise).ok();

    let (rc_ctx, servo) = match build_servo(FlagWaker(wake.clone())) {
        Ok(pair) => pair,
        Err(e) => {
            if let Ok(req) = request_rx.recv() {
                let _ = req.reply.send(Err(e.context("Servo initialization failed")));
            }
            return;
        }
    };

    WAKE.with(|slot| *slot.borrow_mut() = Some(wake.clone()));

    let delegate = Rc::new(SharedDelegate {
        states: RefCell::new(HashMap::new()),
        policy,
    });
    let ucm = Rc::new(UserContentManager::new(&servo));
    ucm.add_stylesheet(Rc::new(create_noise_removal_stylesheet()));

    let mut pending: HashMap<WebViewId, PendingFetch> = HashMap::new();

    loop {
        while let Ok(req) = request_rx.try_recv() {
            accept_request(&servo, &rc_ctx, &delegate, &ucm, req, &mut pending);
        }

        if pending.is_empty() {
            // Idle: block until a new request nudges us or the channel hangs up.
            match request_rx.recv() {
                Ok(req) => accept_request(&servo, &rc_ctx, &delegate, &ucm, req, &mut pending),
                Err(_) => return,
            }
            continue;
        }

        servo.spin_event_loop();
        harvest(&servo, &delegate, &mut pending);

        if !pending.is_empty() {
            // Wait for Servo to wake us or the next pending deadline, whichever is sooner.
            let now = Instant::now();
            let next = pending
                .values()
                .map(|p| {
                    p.state
                        .loaded_at
                        .get()
                        .map_or(p.deadline, |t| t + Duration::from_millis(p.request.settle_ms))
                })
                .min()
                .map_or(FALLBACK_WAIT, |t| t.saturating_duration_since(now).min(FALLBACK_WAIT));
            wake.wait_and_take(next);
        }
    }
}

fn accept_request(
    servo: &servo::Servo,
    rc_ctx: &Rc<SoftwareRenderingContext>,
    delegate: &Rc<SharedDelegate>,
    ucm: &Rc<UserContentManager>,
    req: FetchRequest,
    pending: &mut HashMap<WebViewId, PendingFetch>,
) {
    match start_fetch(servo, rc_ctx, delegate, ucm, req) {
        Ok(p) => {
            pending.insert(p.webview.id(), p);
        }
        Err((req, err)) => {
            let _ = req.reply.send(Err(err));
        }
    }
}

fn harvest(servo: &servo::Servo, delegate: &Rc<SharedDelegate>, pending: &mut HashMap<WebViewId, PendingFetch>) {
    let now = Instant::now();
    let finished: Vec<WebViewId> = pending
        .iter()
        .filter_map(|(id, p)| {
            let settled = p
                .state
                .loaded_at
                .get()
                .is_some_and(|t| now.duration_since(t) >= Duration::from_millis(p.request.settle_ms));
            (settled || now > p.deadline).then_some(*id)
        })
        .collect();

    for id in finished {
        let Some(p) = pending.remove(&id) else { continue };
        let result = finish_fetch(servo, &p);
        delegate.remove(id);
        drop(p.webview);
        let _ = p.request.reply.send(result);
    }
}

fn start_fetch(
    servo: &servo::Servo,
    rc_ctx: &Rc<SoftwareRenderingContext>,
    delegate: &Rc<SharedDelegate>,
    ucm: &Rc<UserContentManager>,
    req: FetchRequest,
) -> std::result::Result<PendingFetch, (FetchRequest, anyhow::Error)> {
    let parsed_url = match url::Url::parse(&req.url) {
        Ok(u) => u,
        Err(e) => return Err((req, anyhow!("bad url: {e}"))),
    };

    let ua = req.user_agent.as_deref().unwrap_or_else(|| default_user_agent());
    servo.set_preference("user_agent", servo::PrefValue::Str(ua.to_owned()));

    let dedicated_ctx = if matches!(req.mode, FetchMode::Screenshot { .. }) {
        let size = PhysicalSize::new(layout::VIEWPORT_WIDTH, layout::VIEWPORT_HEIGHT);
        match SoftwareRenderingContext::new(size) {
            Ok(ctx) => {
                if let Err(e) = ctx.make_current() {
                    return Err((req, anyhow!("failed to make screenshot context current: {e:?}")));
                }
                Some(Rc::new(ctx))
            }
            Err(e) => return Err((req, anyhow!("failed to create screenshot context: {e:?}"))),
        }
    } else {
        None
    };

    let rc_dyn: Rc<dyn RenderingContext> = match dedicated_ctx.as_ref() {
        Some(ctx) => ctx.clone(),
        None => rc_ctx.clone(),
    };

    let delegate_dyn: Rc<dyn WebViewDelegate> = delegate.clone();
    let webview = WebViewBuilder::new(servo, rc_dyn)
        .url(parsed_url)
        .delegate(delegate_dyn)
        .user_content_manager(ucm.clone())
        .build();

    if matches!(req.mode, FetchMode::Content { include_a11y: true }) {
        webview.set_accessibility_active(true);
    }

    let state = delegate.register(webview.id());
    let deadline = Instant::now() + Duration::from_secs(req.timeout_secs);
    Ok(PendingFetch {
        webview,
        request: req,
        deadline,
        state,
        dedicated_ctx,
    })
}

fn finish_fetch(servo: &servo::Servo, p: &PendingFetch) -> Result<ServoPage> {
    let timed_out = p.state.loaded_at.get().is_none() && Instant::now() > p.deadline;

    if let Some(ref ctx) = p.dedicated_ctx {
        let _ = ctx.make_current();
    }

    if !timed_out {
        wait_for_ready_state(servo, &p.webview, p.deadline);
    }

    let html = match eval_js(servo, &p.webview, "document.documentElement.outerHTML") {
        Ok(h) if !h.is_empty() => h,
        _ if timed_out => {
            return Err(anyhow!(
                "page load timed out after {timeout}s (try increasing --timeout)",
                timeout = p.request.timeout_secs,
            ));
        }
        other => other?,
    };

    if timed_out {
        tracing::warn!("page load did not complete; returning best-effort content");
    }
    let inner_text = eval_js(servo, &p.webview, "document.body.innerText").ok();
    let layout_json = eval_js(servo, &p.webview, LAYOUT_JS).ok();

    let (screenshot, js_result) = match &p.request.mode {
        FetchMode::Screenshot { full_page } => (
            crate::screenshot::capture(servo, &p.webview, *full_page, p.request.timeout_secs),
            None,
        ),
        FetchMode::ExecuteJs { expression } => (None, Some(eval_js(servo, &p.webview, expression)?)),
        FetchMode::Content { .. } => (None, None),
    };

    let accessibility_tree = {
        let mut nodes = p.state.a11y_nodes.borrow_mut();
        if nodes.is_empty() {
            None
        } else {
            for node in nodes.values_mut() {
                if node.role() == servo::accesskit::Role::PasswordInput {
                    node.clear_value();
                }
            }
            serde_json::to_string(&*nodes).ok()
        }
    };

    Ok(ServoPage {
        html,
        inner_text,
        layout_json,
        screenshot,
        js_result,
        accessibility_tree,
        console_messages: p.state.console_messages.borrow_mut().drain(..).collect(),
    })
}

fn build_servo(waker: FlagWaker) -> Result<(Rc<SoftwareRenderingContext>, servo::Servo)> {
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

    let rc = Rc::new(ctx);
    let servo = ServoBuilder::default()
        .preferences(prefs)
        .event_loop_waker(Box::new(waker))
        .build();
    Ok((rc, servo))
}

fn create_noise_removal_stylesheet() -> servo::user_contents::UserStyleSheet {
    let url = url::Url::parse("servo-fetch://user-stylesheet/noise-removal").expect("static URL is well-formed");
    servo::user_contents::UserStyleSheet::new(NOISE_REMOVAL_CSS.to_string(), url)
}

/// Wait for `document.readyState` to reach `"complete"`.
///
/// TODO(upstream): Servo's `LoadStatus::Complete` fires before the DOM is
/// fully parsed on pages with heavy inline scripts (e.g. amazon.co.jp); see
/// servo/servo#41972.
fn wait_for_ready_state(servo: &servo::Servo, webview: &WebView, deadline: Instant) {
    while Instant::now() < deadline {
        servo.spin_event_loop();
        if matches!(eval_js(servo, webview, "document.readyState"), Ok(s) if s == "complete") {
            return;
        }
        wait_for_wake(FALLBACK_WAIT);
    }
    tracing::warn!("document did not finish loading; content may be incomplete");
}

pub(crate) fn eval_js(servo: &servo::Servo, webview: &WebView, script: &str) -> Result<String> {
    let result: Rc<RefCell<Option<Result<String>>>> = Rc::new(RefCell::new(None));
    let cb_result = result.clone();

    webview.evaluate_javascript(script, move |js_result| {
        let val = match js_result {
            Ok(JSValue::String(s)) => Ok(s),
            Ok(JSValue::Undefined | JSValue::Null) => Ok(String::new()),
            Ok(JSValue::Boolean(b)) => Ok(b.to_string()),
            Ok(JSValue::Number(n)) => Ok(n.to_string()),
            Ok(other) => jsvalue_to_json(&other).and_then(|v| serde_json::to_string(&v).map_err(|e| anyhow!("{e}"))),
            Err(e) => Err(anyhow!("JS eval error: {e:?}")),
        };
        *cb_result.borrow_mut() = Some(val);
    });

    let deadline = Instant::now() + JS_EVAL_TIMEOUT;
    loop {
        servo.spin_event_loop();
        if let Some(val) = result.borrow_mut().take() {
            return val;
        }
        if Instant::now() > deadline {
            return Err(anyhow!("timeout waiting for JS evaluation"));
        }
        wait_for_wake(FALLBACK_WAIT);
    }
}

fn jsvalue_to_json(val: &JSValue) -> Result<serde_json::Value> {
    const MAX_DEPTH: u8 = 64;
    fn convert(val: &JSValue, depth: u8) -> Result<serde_json::Value> {
        if depth >= MAX_DEPTH {
            return Err(anyhow!("JS value nested too deeply (>{MAX_DEPTH} levels)"));
        }
        Ok(match val {
            JSValue::Undefined | JSValue::Null => serde_json::Value::Null,
            JSValue::Boolean(b) => serde_json::Value::Bool(*b),
            JSValue::Number(n) => serde_json::json!(n),
            JSValue::String(s)
            | JSValue::Element(s)
            | JSValue::ShadowRoot(s)
            | JSValue::Frame(s)
            | JSValue::Window(s) => serde_json::Value::String(s.clone()),
            JSValue::Array(arr) => {
                let items: Result<Vec<_>> = arr.iter().map(|v| convert(v, depth + 1)).collect();
                serde_json::Value::Array(items?)
            }
            JSValue::Object(map) => {
                let entries: Result<serde_json::Map<_, _>> = map
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), convert(v, depth + 1)?)))
                    .collect();
                serde_json::Value::Object(entries?)
            }
        })
    }
    convert(val, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_level_display() {
        assert_eq!(ConsoleLevel::Log.to_string(), "log");
        assert_eq!(ConsoleLevel::Debug.to_string(), "debug");
        assert_eq!(ConsoleLevel::Info.to_string(), "info");
        assert_eq!(ConsoleLevel::Warn.to_string(), "warn");
        assert_eq!(ConsoleLevel::Error.to_string(), "error");
        assert_eq!(ConsoleLevel::Trace.to_string(), "trace");
    }

    #[test]
    fn console_level_serializes_lowercase() {
        let json = serde_json::to_string(&ConsoleLevel::Warn).unwrap();
        assert_eq!(json, "\"warn\"");
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
        assert!(page.screenshot.is_none());
        assert!(page.js_result.is_none());
        assert!(page.accessibility_tree.is_none());
        assert!(page.console_messages.is_empty());
    }

    #[test]
    fn jsvalue_to_json_primitives() {
        assert_eq!(jsvalue_to_json(&JSValue::Null).unwrap(), serde_json::Value::Null);
        assert_eq!(jsvalue_to_json(&JSValue::Undefined).unwrap(), serde_json::Value::Null);
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
        let handle = std::thread::spawn(move || w.wait_and_take(Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(10));
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
        assert!(matches!(ConsoleLevel::from(ConsoleLogLevel::Log), ConsoleLevel::Log));
        assert!(matches!(
            ConsoleLevel::from(ConsoleLogLevel::Debug),
            ConsoleLevel::Debug
        ));
        assert!(matches!(ConsoleLevel::from(ConsoleLogLevel::Info), ConsoleLevel::Info));
        assert!(matches!(ConsoleLevel::from(ConsoleLogLevel::Warn), ConsoleLevel::Warn));
        assert!(matches!(
            ConsoleLevel::from(ConsoleLogLevel::Error),
            ConsoleLevel::Error
        ));
        assert!(matches!(
            ConsoleLevel::from(ConsoleLogLevel::Trace),
            ConsoleLevel::Trace
        ));
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
        let state = WebViewState::default();
        assert!(state.loaded_at.get().is_none(), "loaded_at should be None");
        assert!(!state.a11y_truncated.get(), "a11y_truncated should be false");
        assert!(state.a11y_nodes.borrow().is_empty(), "a11y_nodes should be empty");
        assert!(
            state.console_messages.borrow().is_empty(),
            "console_messages should be empty"
        );
    }
}
