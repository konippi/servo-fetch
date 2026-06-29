//! W3C `WebDriver` engine handle.
//!
//! Unlike the stateless fetch path (one webview per request), `WebDriver` keeps a
//! *persistent* webview per session so commands can act on the same live page:
//! navigate, find an element, click it, read the result, and so on.
//!
//! The single Servo instance is `!Send` and lives on the `servo-engine` thread
//! (see `crate::bridge`). All session state lives there too. [`WebDriverEngine`]
//! is a cheap, cloneable handle that ships closures (`WdTask`) to that thread and
//! blocks on a reply channel — the same pattern the fetch path uses, so it
//! composes with the existing event loop.
//!
//! ## Command routing
//!
//! Servo 0.2's `execute_webdriver_command` forwards everything to the
//! constellation, but the constellation only handles
//! [`servo::WebDriverScriptCommand`]s (find/click/get/script); it panics with
//! `unreachable!("send directly to the embedder")` for embedder-level commands
//! (`LoadUrl`, `TakeScreenshot`, `InputEvent`, window rect, …). So navigation,
//! clicks, screenshots, and window sizing go through the native [`servo::WebView`]
//! embedding API instead, and only script commands round-trip through
//! `execute_webdriver_command`.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use dpi::PhysicalSize;
use euclid::default::Rect as UntypedRect;
use euclid::{Box2D, Point2D};
use image::RgbaImage;
use serde_json::Value;
use servo::{
    CSSPixel, InputEvent, Key, KeyState, KeyboardEvent, LoadStatus, MouseButton, MouseButtonAction, MouseButtonEvent,
    NamedKey, RenderingContext, WebDriverCommandMsg, WebDriverScriptCommand, WebView, WebViewBuilder, WebViewDelegate,
    WebViewId, WebViewPoint, WebViewRect,
};
use servo_base::generic_channel::{GenericSender, TryReceiveError, channel};
use servo_base::id::BrowsingContextId;

use crate::bridge::{self, WdEngineCtx, WdTask, WebViewState, wait_for_wake};

/// Poll granularity while spinning the event loop and waiting on a reply.
const POLL: Duration = Duration::from_millis(10);
/// Transport timeout for a single script-command round-trip to the script thread.
const TRANSPORT_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time to wait for a new session's initial `about:blank` to settle.
const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// How long to keep spinning after a synthetic click/keystroke so handlers run.
const INPUT_SETTLE: Duration = Duration::from_millis(300);
/// Timeout for a screenshot capture.
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(30);
/// Hard cap for an execute-script call whose session script timeout is null.
const SCRIPT_HARD_CAP: Duration = Duration::from_secs(600);

/// Maximum number of concurrent live sessions.
const MAX_SESSIONS: usize = 16;
/// Sessions untouched for this long are garbage-collected on the next new session.
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// W3C default session timeouts.
const DEFAULT_SCRIPT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_PAGE_LOAD_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_IMPLICIT_TIMEOUT_MS: u64 = 0;

/// An opaque `WebDriver` session identifier, returned to clients verbatim.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(format!("servo-fetch-{}", COUNTER.fetch_add(1, Ordering::Relaxed)))
    }

    /// The session id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Element location strategy (the W3C locator strategies servo-fetch supports).
#[derive(Clone, Copy, Debug)]
pub enum Locator {
    /// CSS selector.
    Css,
    /// Exact link text.
    LinkText,
    /// Partial link text.
    PartialLinkText,
    /// Tag name.
    TagName,
    /// `XPath` expression.
    XPath,
}

/// Per-session W3C timeouts, in milliseconds.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    /// Script timeout. `None` means no timeout (explicit JSON null).
    pub script: Option<u64>,
    /// Page-load timeout.
    pub page_load: u64,
    /// Implicit element-retrieval wait.
    pub implicit: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            script: Some(DEFAULT_SCRIPT_TIMEOUT_MS),
            page_load: DEFAULT_PAGE_LOAD_TIMEOUT_MS,
            implicit: DEFAULT_IMPLICIT_TIMEOUT_MS,
        }
    }
}

/// An element's bounding rectangle, in CSS pixels.
#[derive(Clone, Copy, Debug)]
pub struct ElementRect {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// A window's rectangle, in CSS pixels.
#[derive(Clone, Copy, Debug)]
pub struct WindowRect {
    /// X coordinate (always 0 for the headless viewport).
    pub x: i32,
    /// Y coordinate (always 0 for the headless viewport).
    pub y: i32,
    /// Width.
    pub width: i32,
    /// Height.
    pub height: i32,
}

/// Engine-thread state for a single live `WebDriver` session.
pub(crate) struct WdSession {
    webview: WebView,
    webview_id: WebViewId,
    /// Top-level browsing context, derived from the webview id. Script commands
    /// are addressed to this context.
    bcid: BrowsingContextId,
    /// Shared-delegate load-status bookkeeping for this webview.
    state: Rc<WebViewState>,
    timeouts: Cell<Timeouts>,
    last_used: Cell<Instant>,
}

/// Map of live sessions, owned by the `servo-engine` thread.
pub(crate) type SessionMap = HashMap<SessionId, WdSession>;

/// A cheap, cloneable handle to the `WebDriver`-capable Servo engine.
///
/// Methods block the calling thread until the engine thread replies, so call
/// them from a blocking context (e.g. a dedicated request thread).
#[derive(Clone, Default)]
pub struct WebDriverEngine {
    _private: (),
}

impl WebDriverEngine {
    /// Create a handle, starting the Servo engine thread if it is not running.
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Ship a task to the engine thread and block until it replies.
    #[expect(
        clippy::unused_self,
        reason = "the handle is stateless today; methods stay by-ref for a uniform, forward-compatible API"
    )]
    fn dispatch<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut WdEngineCtx<'_>) -> Result<T> + Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Result<T>>(1);
        let task: WdTask = Box::new(move |ctx| {
            let _ = tx.send(f(ctx));
        });
        bridge::submit_wd_task(task)?;
        rx.recv()
            .map_err(|_| anyhow!("Servo engine dropped the WebDriver task (it may have crashed)"))?
    }

    // --- Session lifecycle ---

    /// Create a new session with a fresh, persistent webview.
    #[expect(
        clippy::redundant_closure_for_method_calls,
        reason = "a closure (not a fn reference) is required here for higher-ranked lifetime inference"
    )]
    pub fn new_session(&self) -> Result<SessionId> {
        self.dispatch(|ctx| ctx.new_session())
    }

    /// Tear down a session and drop its webview.
    pub fn delete_session(&self, session: &SessionId) -> Result<()> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.delete_session(&session))
    }

    /// Whether a session is currently live.
    pub fn has_session(&self, session: &SessionId) -> Result<bool> {
        let session = session.clone();
        self.dispatch(move |ctx| Ok(ctx.sessions.contains_key(&session)))
    }

    // --- Navigation ---

    /// Navigate the session's webview to `url` and wait for it to finish loading.
    pub fn navigate(&self, session: &SessionId, url: &str) -> Result<()> {
        let (session, url) = (session.clone(), url.to_string());
        self.dispatch(move |ctx| ctx.navigate(&session, &url))
    }

    /// Current document URL.
    pub fn current_url(&self, session: &SessionId) -> Result<String> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.current_url(&session))
    }

    /// Navigate back in history.
    pub fn back(&self, session: &SessionId) -> Result<()> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.traverse(&session, Traverse::Back))
    }

    /// Navigate forward in history.
    pub fn forward(&self, session: &SessionId) -> Result<()> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.traverse(&session, Traverse::Forward))
    }

    /// Reload the current page.
    pub fn refresh(&self, session: &SessionId) -> Result<()> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.traverse(&session, Traverse::Refresh))
    }

    // --- Document ---

    /// Current document title.
    pub fn title(&self, session: &SessionId) -> Result<String> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.title(&session))
    }

    /// Serialized page source (`document.documentElement.outerHTML`).
    pub fn page_source(&self, session: &SessionId) -> Result<String> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.page_source(&session))
    }

    // --- Locators ---

    /// Find all elements matching a locator, returning their element ids.
    pub fn find_elements(&self, session: &SessionId, locator: Locator, value: &str) -> Result<Vec<String>> {
        let (session, value) = (session.clone(), value.to_string());
        self.dispatch(move |ctx| ctx.find_elements(&session, locator, &value))
    }

    // --- Element getters ---

    /// Visible text of an element.
    pub fn element_text(&self, session: &SessionId, element: &str) -> Result<String> {
        let (session, element) = (session.clone(), element.to_string());
        self.dispatch(move |ctx| ctx.element_text(&session, &element))
    }

    /// An element's HTML attribute value.
    pub fn element_attribute(&self, session: &SessionId, element: &str, name: &str) -> Result<Option<String>> {
        let (session, element, name) = (session.clone(), element.to_string(), name.to_string());
        self.dispatch(move |ctx| ctx.element_attribute(&session, &element, &name))
    }

    /// An element's JavaScript property value.
    pub fn element_property(&self, session: &SessionId, element: &str, name: &str) -> Result<Value> {
        let (session, element, name) = (session.clone(), element.to_string(), name.to_string());
        self.dispatch(move |ctx| ctx.element_property(&session, &element, &name))
    }

    /// An element's computed CSS value.
    pub fn element_css(&self, session: &SessionId, element: &str, name: &str) -> Result<String> {
        let (session, element, name) = (session.clone(), element.to_string(), name.to_string());
        self.dispatch(move |ctx| ctx.element_css(&session, &element, &name))
    }

    /// An element's tag name.
    pub fn element_tag_name(&self, session: &SessionId, element: &str) -> Result<String> {
        let (session, element) = (session.clone(), element.to_string());
        self.dispatch(move |ctx| ctx.element_tag_name(&session, &element))
    }

    /// An element's bounding rectangle.
    pub fn element_rect(&self, session: &SessionId, element: &str) -> Result<ElementRect> {
        let (session, element) = (session.clone(), element.to_string());
        self.dispatch(move |ctx| ctx.element_rect(&session, &element))
    }

    /// Whether an element is enabled.
    pub fn is_enabled(&self, session: &SessionId, element: &str) -> Result<bool> {
        let (session, element) = (session.clone(), element.to_string());
        self.dispatch(move |ctx| ctx.is_enabled(&session, &element))
    }

    /// Whether an element is selected (checkbox/radio/option).
    pub fn is_selected(&self, session: &SessionId, element: &str) -> Result<bool> {
        let (session, element) = (session.clone(), element.to_string());
        self.dispatch(move |ctx| ctx.is_selected(&session, &element))
    }

    /// Whether an element is displayed (approximate W3C displayedness).
    pub fn is_displayed(&self, session: &SessionId, element: &str) -> Result<bool> {
        let (session, element) = (session.clone(), element.to_string());
        self.dispatch(move |ctx| ctx.is_displayed(&session, &element))
    }

    // --- Interactions ---

    /// Click an element (W3C element click).
    pub fn click_element(&self, session: &SessionId, element: &str) -> Result<()> {
        let (session, element) = (session.clone(), element.to_string());
        self.dispatch(move |ctx| ctx.click_element(&session, &element))
    }

    /// Clear an editable element.
    pub fn clear_element(&self, session: &SessionId, element: &str) -> Result<()> {
        let (session, element) = (session.clone(), element.to_string());
        self.dispatch(move |ctx| ctx.clear_element(&session, &element))
    }

    /// Type text into an element.
    pub fn send_keys(&self, session: &SessionId, element: &str, text: &str) -> Result<()> {
        let (session, element, text) = (session.clone(), element.to_string(), text.to_string());
        self.dispatch(move |ctx| ctx.send_keys(&session, &element, &text))
    }

    // --- Scripting ---

    /// Execute a synchronous script and return its JSON result.
    pub fn execute_script(&self, session: &SessionId, script: &str, args: &[Value]) -> Result<Value> {
        let (session, script, args) = (session.clone(), script.to_string(), args.to_vec());
        self.dispatch(move |ctx| ctx.execute_script(&session, &script, &args))
    }

    /// Execute an asynchronous script (the last argument is a completion callback).
    pub fn execute_async_script(&self, session: &SessionId, script: &str, args: &[Value]) -> Result<Value> {
        let (session, script, args) = (session.clone(), script.to_string(), args.to_vec());
        self.dispatch(move |ctx| ctx.execute_async_script(&session, &script, &args))
    }

    // --- Screenshots ---

    /// PNG screenshot of the viewport.
    pub fn screenshot(&self, session: &SessionId) -> Result<Vec<u8>> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.screenshot(&session))
    }

    /// PNG screenshot clipped to an element.
    pub fn element_screenshot(&self, session: &SessionId, element: &str) -> Result<Vec<u8>> {
        let (session, element) = (session.clone(), element.to_string());
        self.dispatch(move |ctx| ctx.element_screenshot(&session, &element))
    }

    // --- Timeouts & window ---

    /// Current session timeouts.
    pub fn get_timeouts(&self, session: &SessionId) -> Result<Timeouts> {
        let session = session.clone();
        self.dispatch(move |ctx| Ok(ctx.session(&session)?.timeouts.get()))
    }

    /// Update session timeouts. Each `Some` value is applied; the outer option of
    /// `script` distinguishes "not provided" (`None`) from "explicit null"
    /// (`Some(None)`, meaning no script timeout).
    pub fn set_timeouts(
        &self,
        session: &SessionId,
        script: Option<Option<u64>>,
        page_load: Option<u64>,
        implicit: Option<u64>,
    ) -> Result<()> {
        let session = session.clone();
        self.dispatch(move |ctx| {
            let s = ctx.session(&session)?;
            let mut t = s.timeouts.get();
            if let Some(script) = script {
                t.script = script;
            }
            if let Some(page_load) = page_load {
                t.page_load = page_load;
            }
            if let Some(implicit) = implicit {
                t.implicit = implicit;
            }
            s.timeouts.set(t);
            Ok(())
        })
    }

    /// Current window rectangle.
    pub fn window_rect(&self, session: &SessionId) -> Result<WindowRect> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.window_rect(&session))
    }

    /// Resize the window (x/y are ignored for the headless viewport).
    pub fn set_window_rect(&self, session: &SessionId, width: Option<i32>, height: Option<i32>) -> Result<WindowRect> {
        let session = session.clone();
        self.dispatch(move |ctx| ctx.set_window_rect(&session, width, height))
    }
}

/// History traversal kind.
#[derive(Clone, Copy)]
enum Traverse {
    Back,
    Forward,
    Refresh,
}

impl WdEngineCtx<'_> {
    fn session(&self, id: &SessionId) -> Result<&WdSession> {
        let session = self
            .sessions
            .get(id)
            .ok_or_else(|| anyhow!("no such WebDriver session: {id}"))?;
        session.last_used.set(Instant::now());
        Ok(session)
    }

    /// Drop sessions untouched for longer than [`SESSION_IDLE_TIMEOUT`]. Run
    /// before every command (see `bridge::accept_message`) so abandoned sessions
    /// are reclaimed on any activity, not only when a new session is requested.
    pub(crate) fn gc_idle_sessions(&mut self) {
        let now = Instant::now();
        let idle: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| now.duration_since(s.last_used.get()) > SESSION_IDLE_TIMEOUT)
            .map(|(id, _)| id.clone())
            .collect();
        for id in idle {
            self.remove_session(&id);
        }
    }

    fn new_session(&mut self) -> Result<SessionId> {
        // Idle sessions were already swept before this command ran; just enforce
        // the live-session ceiling.
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(anyhow!("maximum number of WebDriver sessions ({MAX_SESSIONS}) reached"));
        }

        let rc_dyn: Rc<dyn RenderingContext> = self.rc_ctx.clone();
        let delegate_dyn: Rc<dyn WebViewDelegate> = self.delegate.clone();
        let webview = WebViewBuilder::new(self.servo, rc_dyn)
            .delegate(delegate_dyn)
            .user_content_manager(self.raw_ucm.clone())
            .build();
        webview.focus();
        webview.show();
        let webview_id = webview.id();
        let bcid = BrowsingContextId::from(webview_id);
        let state = self.delegate.register(webview_id, None);

        // Drive the initial about:blank load to completion so that a later
        // `WebView::load` is not dropped before the pipeline is ready.
        let deadline = Instant::now() + READY_TIMEOUT;
        while webview.load_status() != LoadStatus::Complete {
            self.servo.spin_event_loop();
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            wait_for_wake(deadline.saturating_duration_since(now));
        }

        let id = SessionId::next();
        self.sessions.insert(
            id.clone(),
            WdSession {
                webview,
                webview_id,
                bcid,
                state,
                timeouts: Cell::new(Timeouts::default()),
                last_used: Cell::new(Instant::now()),
            },
        );
        Ok(id)
    }

    fn remove_session(&mut self, id: &SessionId) -> Option<WdSession> {
        let session = self.sessions.remove(id)?;
        self.delegate.remove(session.webview_id);
        Some(session)
    }

    fn delete_session(&mut self, id: &SessionId) -> Result<()> {
        let session = self
            .remove_session(id)
            .ok_or_else(|| anyhow!("no such WebDriver session: {id}"))?;
        drop(session.webview);
        Ok(())
    }

    fn navigate(&self, id: &SessionId, url: &str) -> Result<()> {
        // Validate the scheme and SSRF policy up front, exactly like the fetch
        // path — this fails fast on disallowed/loopback/`file:` URLs instead of
        // loading them (or hanging until the page-load timeout if the navigation
        // delegate later denies them).
        let parsed = crate::net::validate_url(url)
            .map_err(|e| anyhow!("InvalidArgument: invalid or disallowed URL '{url}': {e}"))?;
        let session = self.session(id)?;
        session.webview.load(parsed);
        self.await_load(session)
    }

    fn traverse(&self, id: &SessionId, kind: Traverse) -> Result<()> {
        let session = self.session(id)?;
        match kind {
            Traverse::Back => {
                if !session.webview.can_go_back() {
                    return Ok(());
                }
                session.webview.go_back(1);
            }
            Traverse::Forward => {
                if !session.webview.can_go_forward() {
                    return Ok(());
                }
                session.webview.go_forward(1);
            }
            Traverse::Refresh => session.webview.reload(),
        }
        self.await_load(session)
    }

    /// Clear the load marker, then spin until the next load completes.
    fn await_load(&self, session: &WdSession) -> Result<()> {
        session.state.loaded_at.set(None);
        let deadline = Instant::now() + Duration::from_millis(session.timeouts.get().page_load);
        loop {
            self.servo.spin_event_loop();
            if session.state.loaded_at.get().is_some() {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(anyhow!("navigation timed out"));
            }
            wait_for_wake(deadline.saturating_duration_since(now));
        }
    }

    fn current_url(&self, id: &SessionId) -> Result<String> {
        let bcid = self.session(id)?.bcid;
        script_command(self.servo, bcid, transport_deadline(), WebDriverScriptCommand::GetUrl)
    }

    fn title(&self, id: &SessionId) -> Result<String> {
        let bcid = self.session(id)?.bcid;
        script_command(self.servo, bcid, transport_deadline(), WebDriverScriptCommand::GetTitle)
    }

    fn page_source(&self, id: &SessionId) -> Result<String> {
        let bcid = self.session(id)?.bcid;
        script_command(
            self.servo,
            bcid,
            transport_deadline(),
            WebDriverScriptCommand::GetPageSource,
        )?
        .map_err(|e| script_error("get page source", e))
    }

    fn find_elements(&self, id: &SessionId, locator: Locator, value: &str) -> Result<Vec<String>> {
        let bcid = self.session(id)?.bcid;
        let dl = transport_deadline();
        let v = value.to_string();
        let found = match locator {
            Locator::Css => script_command(self.servo, bcid, dl, |tx| {
                WebDriverScriptCommand::FindElementsCSSSelector(v, tx)
            }),
            Locator::LinkText => script_command(self.servo, bcid, dl, |tx| {
                WebDriverScriptCommand::FindElementsLinkText(v, false, tx)
            }),
            Locator::PartialLinkText => script_command(self.servo, bcid, dl, |tx| {
                WebDriverScriptCommand::FindElementsLinkText(v, true, tx)
            }),
            Locator::TagName => script_command(self.servo, bcid, dl, |tx| {
                WebDriverScriptCommand::FindElementsTagName(v, tx)
            }),
            Locator::XPath => script_command(self.servo, bcid, dl, |tx| {
                WebDriverScriptCommand::FindElementsXpathSelector(v, tx)
            }),
        }?;
        found.map_err(|e| script_error("find elements", e))
    }

    fn element_text(&self, id: &SessionId, element: &str) -> Result<String> {
        let bcid = self.session(id)?.bcid;
        let element = element.to_string();
        script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::GetElementText(element, tx)
        })?
        .map_err(|e| script_error("get element text", e))
    }

    fn element_attribute(&self, id: &SessionId, element: &str, name: &str) -> Result<Option<String>> {
        let bcid = self.session(id)?.bcid;
        let (element, name) = (element.to_string(), name.to_string());
        script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::GetElementAttribute(element, name, tx)
        })?
        .map_err(|e| script_error("get element attribute", e))
    }

    fn element_property(&self, id: &SessionId, element: &str, name: &str) -> Result<Value> {
        let bcid = self.session(id)?.bcid;
        let (element, name) = (element.to_string(), name.to_string());
        let value = script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::GetElementProperty(element, name, tx)
        })?
        .map_err(|e| script_error("get element property", e))?;
        bridge::jsvalue_to_json(&value)
    }

    fn element_css(&self, id: &SessionId, element: &str, name: &str) -> Result<String> {
        let bcid = self.session(id)?.bcid;
        let (element, name) = (element.to_string(), name.to_string());
        script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::GetElementCSS(element, name, tx)
        })?
        .map_err(|e| script_error("get element css", e))
    }

    fn element_tag_name(&self, id: &SessionId, element: &str) -> Result<String> {
        let bcid = self.session(id)?.bcid;
        let element = element.to_string();
        script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::GetElementTagName(element, tx)
        })?
        .map_err(|e| script_error("get element tag name", e))
    }

    fn raw_element_rect(&self, bcid: BrowsingContextId, element: &str) -> Result<UntypedRect<f64>> {
        let element = element.to_string();
        script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::GetElementRect(element, tx)
        })?
        .map_err(|e| script_error("get element rect", e))
    }

    fn element_rect(&self, id: &SessionId, element: &str) -> Result<ElementRect> {
        let bcid = self.session(id)?.bcid;
        let rect = self.raw_element_rect(bcid, element)?;
        Ok(ElementRect {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        })
    }

    fn is_enabled(&self, id: &SessionId, element: &str) -> Result<bool> {
        let bcid = self.session(id)?.bcid;
        let element = element.to_string();
        script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::IsEnabled(element, tx)
        })?
        .map_err(|e| script_error("is enabled", e))
    }

    fn is_selected(&self, id: &SessionId, element: &str) -> Result<bool> {
        let bcid = self.session(id)?.bcid;
        let element = element.to_string();
        script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::IsSelected(element, tx)
        })?
        .map_err(|e| script_error("is selected", e))
    }

    /// Approximate the W3C "is element displayed" check from geometry and
    /// computed style (Servo exposes no dedicated script command for it).
    fn is_displayed(&self, id: &SessionId, element: &str) -> Result<bool> {
        let bcid = self.session(id)?.bcid;
        // A `display:none` or zero-area element has an empty bounding rect, so
        // this single round-trip subsumes the separate `display` check.
        let rect = self.raw_element_rect(bcid, element)?;
        if rect.size.width <= 0.0 && rect.size.height <= 0.0 {
            return Ok(false);
        }
        let css = |property: &str| -> Result<String> {
            let (element, property) = (element.to_string(), property.to_string());
            script_command(self.servo, bcid, transport_deadline(), |tx| {
                WebDriverScriptCommand::GetElementCSS(element, property, tx)
            })?
            .map_err(|e| script_error("get element css", e))
        };
        if matches!(css("visibility")?.as_str(), "hidden" | "collapse") {
            return Ok(false);
        }
        if css("opacity")?.parse::<f64>().is_ok_and(|o| o == 0.0) {
            return Ok(false);
        }
        Ok(true)
    }

    /// Scroll the element into view and return its viewport-relative rect (CSS
    /// pixels). Channel-based, so it is bounded by the transport deadline.
    fn scroll_and_rect(&self, bcid: BrowsingContextId, element: &str) -> Result<UntypedRect<f32>> {
        let element = element.to_string();
        script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::ScrollAndGetBoundingClientRect(element, tx)
        })?
        .map_err(|e| script_error("scroll and get element rect", e))
    }

    fn click_element(&self, id: &SessionId, element: &str) -> Result<()> {
        let bcid = self.session(id)?.bcid;

        // The script side performs <option> selection itself (returns `None`)
        // and otherwise scrolls the element into view and returns it for the
        // embedder to click at its in-view center point.
        let element_owned = element.to_string();
        let outcome = script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::ElementClick(element_owned, tx)
        })?
        .map_err(|e| script_error("element click", e))?;
        if outcome.is_none() {
            return Ok(());
        }

        // Compute the viewport-relative center to click. `ScrollAndGetBoundingClientRect`
        // scrolls the element into view and returns its `getBoundingClientRect()`,
        // which is already viewport-relative — exactly what `WebViewPoint::Page` wants.
        let rect = self.scroll_and_rect(bcid, element)?;
        let point = css_point(
            rect.origin.x + rect.size.width / 2.0,
            rect.origin.y + rect.size.height / 2.0,
        );
        let webview = &self.session(id)?.webview;
        webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
            MouseButtonAction::Down,
            MouseButton::Left,
            point,
        )));
        webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
            MouseButtonAction::Up,
            MouseButton::Left,
            point,
        )));
        self.settle(INPUT_SETTLE);
        Ok(())
    }

    fn clear_element(&self, id: &SessionId, element: &str) -> Result<()> {
        let bcid = self.session(id)?.bcid;
        let element = element.to_string();
        script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::ElementClear(element, tx)
        })?
        .map_err(|e| script_error("element clear", e))
    }

    fn send_keys(&self, id: &SessionId, element: &str, text: &str) -> Result<()> {
        let bcid = self.session(id)?.bcid;
        // WillSendKeys focuses the element and returns whether the embedder should
        // dispatch keyboard events (false for file inputs, handled in script).
        let element_owned = element.to_string();
        let text_owned = text.to_string();
        let should_send = script_command(self.servo, bcid, transport_deadline(), |tx| {
            WebDriverScriptCommand::WillSendKeys(element_owned, text_owned, false, tx)
        })?
        .map_err(|e| script_error("send keys", e))?;
        if !should_send {
            return Ok(());
        }

        let webview = &self.session(id)?.webview;
        for ch in text.chars() {
            let key = map_key(ch);
            webview.notify_input_event(InputEvent::Keyboard(KeyboardEvent::from_state_and_key(
                KeyState::Down,
                key.clone(),
            )));
            webview.notify_input_event(InputEvent::Keyboard(KeyboardEvent::from_state_and_key(
                KeyState::Up,
                key,
            )));
        }
        self.settle(INPUT_SETTLE);
        Ok(())
    }

    fn execute_script(&self, id: &SessionId, script: &str, args: &[Value]) -> Result<Value> {
        let session = self.session(id)?;
        let deadline = script_deadline(session);
        let wrapped = wrap_sync_script(script, args);
        bridge::eval_json(self.servo, &session.webview, &wrapped, deadline)
    }

    fn execute_async_script(&self, id: &SessionId, script: &str, args: &[Value]) -> Result<Value> {
        let session = self.session(id)?;
        let deadline = script_deadline(session);
        let webview = &session.webview;

        let kickoff = wrap_async_script(script, args);
        bridge::eval_json(self.servo, webview, &kickoff, transport_deadline())?;

        loop {
            let done = bridge::eval_json(
                self.servo,
                webview,
                "!!(window.__servo_fetch_async && window.__servo_fetch_async.done)",
                transport_deadline(),
            )?;
            if done.as_bool() == Some(true) {
                break;
            }
            if Instant::now() >= deadline {
                return Err(anyhow!("async script timed out"));
            }
            wait_for_wake(POLL);
        }

        let error = bridge::eval_json(
            self.servo,
            webview,
            "(window.__servo_fetch_async && window.__servo_fetch_async.error) || null",
            transport_deadline(),
        )?;
        if let Some(message) = error.as_str() {
            return Err(anyhow!("async script error: {message}"));
        }
        bridge::eval_json(
            self.servo,
            webview,
            "(window.__servo_fetch_async ? window.__servo_fetch_async.value : null)",
            transport_deadline(),
        )
    }

    fn screenshot(&self, id: &SessionId) -> Result<Vec<u8>> {
        let webview = &self.session(id)?.webview;
        let deadline = Instant::now() + SCREENSHOT_TIMEOUT;
        let image = crate::screenshot::capture(self.servo, webview, false, deadline)
            .ok_or_else(|| anyhow!("screenshot capture failed"))?;
        encode_png(&image)
    }

    fn element_screenshot(&self, id: &SessionId, element: &str) -> Result<Vec<u8>> {
        let bcid = self.session(id)?.bcid;
        // Viewport-relative rect (also scrolls the element into view), so the
        // clip region is correct even when the page is scrolled.
        let rect = self.scroll_and_rect(bcid, element)?;
        let webview = &self.session(id)?.webview;
        let deadline = Instant::now() + SCREENSHOT_TIMEOUT;
        let view_rect = WebViewRect::Page(page_box(rect));
        let image = crate::screenshot::take_screenshot(self.servo, webview, Some(view_rect), deadline)
            .ok_or_else(|| anyhow!("element screenshot capture failed"))?;
        encode_png(&image)
    }

    fn window_rect(&self, id: &SessionId) -> Result<WindowRect> {
        let size = self.session(id)?.webview.size();
        Ok(WindowRect {
            x: 0,
            y: 0,
            width: round_to_i32(size.width),
            height: round_to_i32(size.height),
        })
    }

    fn set_window_rect(&self, id: &SessionId, width: Option<i32>, height: Option<i32>) -> Result<WindowRect> {
        let webview = &self.session(id)?.webview;
        let current = webview.size();
        let new_width = width.map_or_else(|| round_to_i32(current.width), |w| w.max(0));
        let new_height = height.map_or_else(|| round_to_i32(current.height), |h| h.max(0));
        webview.resize(PhysicalSize::new(to_u32(new_width), to_u32(new_height)));
        self.settle(INPUT_SETTLE);
        Ok(WindowRect {
            x: 0,
            y: 0,
            width: new_width,
            height: new_height,
        })
    }

    /// Spin the event loop for `duration` so async page work can progress.
    fn settle(&self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.servo.spin_event_loop();
            wait_for_wake(POLL);
        }
    }
}

fn transport_deadline() -> Instant {
    Instant::now() + TRANSPORT_TIMEOUT
}

/// Deadline for an execute-script call, honoring the session script timeout.
fn script_deadline(session: &WdSession) -> Instant {
    let cap = session
        .timeouts
        .get()
        .script
        .map_or(SCRIPT_HARD_CAP, Duration::from_millis);
    Instant::now() + cap.min(SCRIPT_HARD_CAP)
}

/// Dispatch a script command to a browsing context and spin the event loop until
/// it replies (or the deadline elapses). Used for the channel-based commands.
fn script_command<T, F>(servo: &servo::Servo, bcid: BrowsingContextId, deadline: Instant, build: F) -> Result<T>
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de> + Send + 'static,
    F: FnOnce(GenericSender<T>) -> WebDriverScriptCommand,
{
    let (tx, rx) = channel::<T>().ok_or_else(|| anyhow!("failed to create WebDriver reply channel"))?;
    servo.execute_webdriver_command(WebDriverCommandMsg::ScriptCommand(bcid, build(tx)));
    loop {
        servo.spin_event_loop();
        match rx.try_recv_timeout(POLL) {
            Ok(value) => return Ok(value),
            Err(TryReceiveError::Empty) => {
                if Instant::now() >= deadline {
                    return Err(anyhow!("WebDriver script command timed out"));
                }
            }
            Err(TryReceiveError::ReceiveError(e)) => {
                return Err(anyhow!("WebDriver reply channel closed: {e}"));
            }
        }
    }
}

/// Format a script-command failure so its first token is the W3C error-status
/// name (e.g. `NoSuchElement`), letting the CLI handler map it back to an
/// `ErrorStatus` precisely without depending on the `webdriver` crate here.
fn script_error<E: std::fmt::Debug>(context: &str, status: E) -> anyhow::Error {
    anyhow!("{status:?}: {context} failed")
}

/// W3C "execute script": wrap the body as a function applied to its arguments.
fn wrap_sync_script(body: &str, args: &[Value]) -> String {
    let args_json = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_string());
    format!("(function() {{ {body} }}).apply(window, {args_json});")
}

/// W3C "execute async script": run the body with a completion callback appended
/// to its arguments, stashing the result on a well-known global for polling.
fn wrap_async_script(body: &str, args: &[Value]) -> String {
    let args_json = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_string());
    format!(
        "(function() {{
            window.__servo_fetch_async = {{ done: false, value: null, error: null }};
            try {{
                var __args = {args_json};
                __args.push(function(v) {{
                    try {{ window.__servo_fetch_async.value = v; }} catch (e) {{}}
                    window.__servo_fetch_async.done = true;
                }});
                (function() {{ {body} }}).apply(window, __args);
            }} catch (e) {{
                window.__servo_fetch_async.error = (e && e.message) ? String(e.message) : String(e);
                window.__servo_fetch_async.done = true;
            }}
        }})();"
    )
}

/// Map a character to a Servo key, translating the W3C special-key codepoints
/// (U+E0xx) for the few keys most automation uses.
fn map_key(ch: char) -> Key {
    match ch {
        '\u{E003}' => Key::Named(NamedKey::Backspace),
        '\u{E004}' => Key::Named(NamedKey::Tab),
        '\u{E006}' | '\u{E007}' => Key::Named(NamedKey::Enter),
        '\u{E00C}' => Key::Named(NamedKey::Escape),
        '\u{E017}' => Key::Named(NamedKey::Delete),
        other => Key::Character(other.to_string()),
    }
}

fn css_point(x: f32, y: f32) -> WebViewPoint {
    Point2D::<f32, CSSPixel>::new(x, y).into()
}

fn page_box(rect: UntypedRect<f32>) -> Box2D<f32, CSSPixel> {
    let min = Point2D::new(rect.origin.x, rect.origin.y);
    let max = Point2D::new(rect.origin.x + rect.size.width, rect.origin.y + rect.size.height);
    Box2D::new(min, max)
}

#[expect(clippy::cast_possible_truncation, reason = "viewport dimensions are small values")]
fn round_to_i32(v: f32) -> i32 {
    v.round() as i32
}

#[expect(clippy::cast_sign_loss, reason = "value is clamped to be non-negative by callers")]
fn to_u32(v: i32) -> u32 {
    v.max(0) as u32
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| anyhow!("PNG encoding failed: {e}"))?;
    Ok(buf.into_inner())
}
