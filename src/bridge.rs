//! Servo engine bridge — persistent Servo thread with channel-based communication.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use dpi::PhysicalSize;
use euclid::{Box2D, Point2D};
use image::RgbaImage;
use servo::{
    ConsoleLogLevel, JSValue, LoadStatus, NavigationRequest, Preferences, RenderingContext, ServoBuilder,
    SoftwareRenderingContext, UserContentManager, WebView, WebViewBuilder, WebViewDelegate,
};

use servo_fetch::layout;

const JS_EVAL_TIMEOUT: Duration = Duration::from_secs(10);
const SETTLE_DURATION: Duration = Duration::from_millis(500);
const SPIN_INTERVAL: Duration = Duration::from_millis(10);
const LAYOUT_JS: &str = include_str!("js/layout.js");
const MAX_PDF_BYTES: u64 = 50 * 1024 * 1024;
const MAX_CONSOLE_MESSAGES: usize = 100;

/// CSS injected via user stylesheets to strip common noise elements.
/// Uses the user origin with `!important` to override author styles.
const NOISE_REMOVAL_CSS: &str = "\
[aria-label*=\"cookie\" i], [aria-label*=\"consent\" i], \
[class*=\"cookie-banner\" i], [class*=\"cookie-consent\" i], \
[id*=\"cookie\" i][class*=\"banner\" i], \
[class*=\"newsletter-popup\" i], [class*=\"subscribe-modal\" i] \
{ display: none !important; }";

struct Delegate {
    loaded: Rc<Cell<bool>>,
    pdf_data: Rc<RefCell<Option<Vec<u8>>>>,
    a11y_nodes: Rc<RefCell<HashMap<servo::accesskit::NodeId, servo::accesskit::Node>>>,
    console_messages: Rc<RefCell<Vec<ConsoleMessage>>>,
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

impl WebViewDelegate for Delegate {
    fn notify_load_status_changed(&self, _webview: WebView, status: LoadStatus) {
        if status == LoadStatus::Complete {
            self.loaded.set(true);
        }
    }

    fn notify_new_frame_ready(&self, webview: WebView) {
        webview.paint();
    }

    fn request_navigation(&self, _webview: WebView, navigation_request: NavigationRequest) {
        let is_http = matches!(navigation_request.url.scheme(), "http" | "https");
        match navigation_request.url.host_str() {
            Some(host) if is_http && !crate::net::is_private_host(host) => navigation_request.allow(),
            _ => {
                eprintln!("warning: blocked navigation to {}", navigation_request.url);
                navigation_request.deny();
            }
        }
    }

    fn notify_accessibility_tree_update(&self, _webview: WebView, tree_update: servo::accesskit::TreeUpdate) {
        // TreeUpdate is incremental — merge nodes into a single map to build the full tree.
        let mut nodes = self.a11y_nodes.borrow_mut();
        for (id, node) in tree_update.nodes {
            nodes.insert(id, node);
        }
    }

    fn show_console_message(&self, _webview: WebView, level: ConsoleLogLevel, message: String) {
        let mut msgs = self.console_messages.borrow_mut();
        if msgs.len() < MAX_CONSOLE_MESSAGES {
            let level = match level {
                ConsoleLogLevel::Log => ConsoleLevel::Log,
                ConsoleLogLevel::Debug => ConsoleLevel::Debug,
                ConsoleLogLevel::Info => ConsoleLevel::Info,
                ConsoleLogLevel::Warn => ConsoleLevel::Warn,
                ConsoleLogLevel::Error => ConsoleLevel::Error,
                ConsoleLogLevel::Trace => ConsoleLevel::Trace,
            };
            msgs.push(ConsoleMessage { level, message });
        }
    }

    fn load_web_resource(&self, _webview: WebView, load: servo::WebResourceLoad) {
        let request = load.request();
        if !request.is_for_main_frame {
            return;
        }

        let url = request.url.clone();

        // SSRF check: validate the host before making any request.
        if let Some(host) = url.host_str() {
            if crate::net::is_private_host(host) {
                return;
            }
        }

        // HEAD request to check Content-Type without downloading the body.
        // Disable redirects to prevent SSRF via redirect to private IPs.
        let agent = ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .max_redirects(0)
                .timeout_global(Some(std::time::Duration::from_secs(15)))
                .build(),
        );
        let Ok(head_resp) = agent.head(url.as_str()).call() else {
            return;
        };

        let is_pdf = head_resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.to_ascii_lowercase().starts_with("application/pdf"));

        if !is_pdf {
            return; // Not a PDF — let Servo handle normally.
        }

        // Fetch the PDF body with a size limit.
        let Ok(get_resp) = agent.get(url.as_str()).call() else {
            return;
        };
        let Ok(bytes) = get_resp.into_body().with_config().limit(MAX_PDF_BYTES).read_to_vec() else {
            return;
        };

        *self.pdf_data.borrow_mut() = Some(bytes);

        // Send empty HTML so Servo completes loading.
        let resp = servo::WebResourceResponse::new(url);
        let mut intercepted = load.intercept(resp);
        intercepted.send_body_data(b"<html><body></body></html>".to_vec());
        intercepted.finish();
    }
}

#[derive(Default)]
pub(crate) struct ServoPage {
    pub html: String,
    pub inner_text: Option<String>,
    pub layout_json: Option<String>,
    pub screenshot: Option<RgbaImage>,
    pub js_result: Option<String>,
    pub pdf_data: Option<Vec<u8>>,
    pub accessibility_tree: Option<String>,
    pub console_messages: Vec<ConsoleMessage>,
}

/// Options for fetching a page via Servo.
pub(crate) struct FetchOptions<'a> {
    pub url: &'a str,
    pub timeout_secs: u64,
    pub screenshot: bool,
    pub accessibility_tree: bool,
    pub js: Option<&'a str>,
}

struct FetchRequest {
    url: String,
    timeout_secs: u64,
    take_screenshot: bool,
    need_a11y: bool,
    custom_js: Option<String>,
    reply: mpsc::Sender<Result<ServoPage>>,
}

/// Fetch a page via Servo. First call spawns a persistent Servo thread.
pub(crate) fn fetch_page(opts: &FetchOptions<'_>) -> Result<ServoPage> {
    static SENDER: std::sync::OnceLock<mpsc::Sender<FetchRequest>> = std::sync::OnceLock::new();

    // Suppress stderr before Servo init to avoid OpenGL driver noise.
    let guard = stderr_guard::StderrGuard::suppress();

    let sender = SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<FetchRequest>();
        std::thread::Builder::new()
            .name("servo-engine".into())
            .spawn(move || {
                servo_thread(rx);
            })
            .expect("failed to spawn servo thread");
        tx
    });

    let (reply_tx, reply_rx) = mpsc::channel();
    sender
        .send(FetchRequest {
            url: opts.url.to_string(),
            timeout_secs: opts.timeout_secs,
            take_screenshot: opts.screenshot,
            need_a11y: opts.accessibility_tree,
            custom_js: opts.js.map(String::from),
            reply: reply_tx,
        })
        .map_err(|_| anyhow!("Servo engine is not running (it may have crashed on a previous request)"))?;

    let result = reply_rx
        .recv()
        .map_err(|_| anyhow!("Servo engine crashed while processing this page. Try a different URL."))?;

    drop(guard);

    result
}

#[expect(clippy::needless_pass_by_value)]
fn servo_thread(rx: mpsc::Receiver<FetchRequest>) {
    let (rc_ctx, servo) = match build_servo() {
        Ok(pair) => pair,
        Err(e) => {
            if let Ok(req) = rx.recv() {
                let _ = req.reply.send(Err(e.context("Servo initialization failed")));
            }
            return;
        }
    };

    while let Ok(req) = rx.recv() {
        let rc_dyn: Rc<dyn RenderingContext> = rc_ctx.clone();
        let loaded = Rc::new(Cell::new(false));
        let pdf_data: Rc<RefCell<Option<Vec<u8>>>> = Rc::new(RefCell::new(None));
        let a11y_nodes: Rc<RefCell<HashMap<servo::accesskit::NodeId, servo::accesskit::Node>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let console_messages: Rc<RefCell<Vec<ConsoleMessage>>> = Rc::new(RefCell::new(Vec::new()));
        let delegate = Rc::new(Delegate {
            loaded: loaded.clone(),
            pdf_data: pdf_data.clone(),
            a11y_nodes: a11y_nodes.clone(),
            console_messages: console_messages.clone(),
        });

        let parsed_url = match url::Url::parse(&req.url) {
            Ok(u) => u,
            Err(e) => {
                let _ = req.reply.send(Err(anyhow!("bad url: {e}")));
                continue;
            }
        };

        // Set up user stylesheets for noise removal.
        let ucm = Rc::new(UserContentManager::new(&servo));
        if let Some(stylesheet) = create_noise_removal_stylesheet() {
            ucm.add_stylesheet(Rc::new(stylesheet));
        }

        let webview = WebViewBuilder::new(&servo, rc_dyn)
            .url(parsed_url)
            .delegate(delegate)
            .user_content_manager(ucm)
            .build();

        // Only enable accessibility tree collection when explicitly requested,
        // to avoid the overhead of building the a11y tree on every fetch.
        if req.need_a11y {
            webview.set_accessibility_active(true);
        }

        let result = handle_request(
            &servo,
            &webview,
            &rc_ctx,
            &loaded,
            &pdf_data,
            &a11y_nodes,
            &console_messages,
            &req,
        );
        drop(webview);
        let _ = req.reply.send(result);
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_request(
    servo: &servo::Servo,
    webview: &WebView,
    rc_ctx: &Rc<SoftwareRenderingContext>,
    loaded: &Cell<bool>,
    pdf_data: &RefCell<Option<Vec<u8>>>,
    a11y_nodes: &RefCell<HashMap<servo::accesskit::NodeId, servo::accesskit::Node>>,
    console_messages: &RefCell<Vec<ConsoleMessage>>,
    req: &FetchRequest,
) -> Result<ServoPage> {
    let deadline = Instant::now() + Duration::from_secs(req.timeout_secs);
    spin_until(servo, loaded, deadline, req.timeout_secs)?;

    let html = eval_js(servo, webview, "document.documentElement.outerHTML")?;
    let inner_text = eval_js(servo, webview, "document.body.innerText").ok();
    let layout_json = eval_js(servo, webview, LAYOUT_JS).ok();

    #[expect(clippy::cast_possible_wrap)]
    let screenshot = if req.take_screenshot {
        let rect = Box2D::new(
            Point2D::new(0, 0),
            Point2D::new(layout::VIEWPORT_WIDTH as i32, layout::VIEWPORT_HEIGHT as i32),
        );
        rc_ctx.read_to_image(rect)
    } else {
        None
    };

    let js_result = req
        .custom_js
        .as_deref()
        .map(|expr| eval_js(servo, webview, expr))
        .transpose()?;

    // Serialize the merged accessibility tree, masking password field values.
    let accessibility_tree = {
        let mut nodes = a11y_nodes.borrow_mut();
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
        pdf_data: pdf_data.borrow_mut().take(),
        accessibility_tree,
        console_messages: console_messages.borrow_mut().drain(..).collect(),
    })
}

fn build_servo() -> Result<(Rc<SoftwareRenderingContext>, servo::Servo)> {
    let size = PhysicalSize::new(layout::VIEWPORT_WIDTH, layout::VIEWPORT_HEIGHT);
    let ctx = SoftwareRenderingContext::new(size).map_err(|e| anyhow!("failed to create rendering context: {e:?}"))?;
    ctx.make_current()
        .map_err(|e| anyhow!("failed to make context current: {e:?}"))?;

    let prefs = Preferences {
        accessibility_enabled: true,
        dom_webgpu_enabled: false,
        dom_webxr_enabled: false,
        dom_serviceworker_enabled: false,
        dom_bluetooth_enabled: false,
        ..Preferences::default()
    };

    let rc = Rc::new(ctx);
    let servo = ServoBuilder::default().preferences(prefs).build();
    Ok((rc, servo))
}

/// Create a user stylesheet for noise removal. Uses serde deserialization because
/// `UserStyleSheet` does not expose a public constructor in servo v0.1.0.
fn create_noise_removal_stylesheet() -> Option<servo::user_contents::UserStyleSheet> {
    let result = serde_json::from_value(serde_json::json!({
        "source": NOISE_REMOVAL_CSS,
        "url": "servo-fetch://user-stylesheet/noise-removal"
    }));
    match result {
        Ok(stylesheet) => Some(stylesheet),
        Err(e) => {
            eprintln!("warning: failed to create noise removal stylesheet: {e}");
            None
        }
    }
}

fn spin_until(servo: &servo::Servo, condition: &Cell<bool>, deadline: Instant, timeout_secs: u64) -> Result<()> {
    while !condition.get() {
        if Instant::now() > deadline {
            return Err(anyhow!(
                "page load timed out after {timeout_secs}s (try increasing --timeout)"
            ));
        }
        servo.spin_event_loop();
        std::thread::sleep(SPIN_INTERVAL);
    }
    let settle_end = Instant::now() + SETTLE_DURATION;
    while Instant::now() < settle_end {
        servo.spin_event_loop();
        std::thread::sleep(SPIN_INTERVAL);
    }
    Ok(())
}

fn eval_js(servo: &servo::Servo, webview: &WebView, script: &str) -> Result<String> {
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
        std::thread::sleep(SPIN_INTERVAL);
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

// macOS's Apple Silicon OpenGL driver writes noise to fd 2 via fprintf.
// Temporarily redirect fd 2 → /dev/null using POSIX dup/dup2 save-restore.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_level_display() {
        assert_eq!(ConsoleLevel::Log.to_string(), "log");
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
        assert!(page.screenshot.is_none());
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
}

// macOS's Apple Silicon OpenGL driver writes noise to fd 2 via fprintf.
// Temporarily redirect fd 2 → /dev/null using POSIX dup/dup2 save-restore.
#[cfg(unix)]
mod stderr_guard {
    use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};

    /// RAII guard: suppresses stderr on creation, restores on drop.
    pub(crate) struct StderrGuard {
        saved_fd: Option<OwnedFd>,
    }

    impl StderrGuard {
        #[allow(unsafe_code)]
        pub(crate) fn suppress() -> Self {
            // SAFETY: dup/dup2/fcntl/close are standard POSIX calls.
            let saved = unsafe { libc::dup(2) };
            if saved < 0 {
                return Self { saved_fd: None };
            }
            let saved_fd = unsafe { OwnedFd::from_raw_fd(saved) };
            unsafe { libc::fcntl(saved_fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };

            let Ok(devnull) = std::fs::File::open("/dev/null") else {
                return Self { saved_fd: None };
            };
            let null_fd = devnull.into_raw_fd();
            unsafe {
                libc::dup2(null_fd, 2);
                libc::close(null_fd);
            }
            Self {
                saved_fd: Some(saved_fd),
            }
        }
    }

    impl Drop for StderrGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            if let Some(ref fd) = self.saved_fd {
                unsafe {
                    libc::dup2(fd.as_raw_fd(), 2);
                }
            }
        }
    }
}

#[cfg(not(unix))]
mod stderr_guard {
    pub(crate) struct StderrGuard;
    impl StderrGuard {
        pub(crate) fn suppress() -> Self {
            Self
        }
    }
}
