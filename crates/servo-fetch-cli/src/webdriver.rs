//! W3C `WebDriver` HTTP server.
//!
//! Implements the `webdriver` crate's [`WebDriverHandler`] by mapping each W3C
//! command onto [`servo_fetch::webdriver::WebDriverEngine`]. The engine owns the
//! Servo session state on its own thread; this layer is the protocol glue plus
//! the bits the W3C spec puts on the *server* side (implicit-wait retries for
//! element lookup, capability negotiation, error-status mapping).

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use base64::Engine as _;
use serde_json::{Value, json};
use servo_fetch::webdriver::{ElementRect, Locator, SessionId, Timeouts, WebDriverEngine, WindowRect};
use tokio::signal;
use webdriver::command::{LocatorParameters, WebDriverCommand, WebDriverMessage};
use webdriver::common::{LocatorStrategy, WebElement};
use webdriver::error::{ErrorStatus, WebDriverError, WebDriverResult};
use webdriver::httpapi::VoidWebDriverExtensionRoute;
use webdriver::response::{
    ElementRectResponse, NewSessionResponse, TimeoutsResponse, ValueResponse, WebDriverResponse, WindowRectResponse,
};
use webdriver::server::{Session, SessionTeardownKind, WebDriverHandler};

/// Poll interval for the implicit-wait element-retrieval loop.
const FIND_POLL: Duration = Duration::from_millis(50);

/// Start the `WebDriver` server on `host:port` and run until interrupted.
pub(crate) async fn run(host: &str, port: u16) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let handler = ServoWdHandler::new();

    // Accept `Host: localhost` (W3C clients' default) in addition to the
    // always-allowed IP-literal hosts; reject everything else as an SSRF guard.
    let allow_hosts = vec![url::Host::Domain("localhost".to_string())];

    let listener = webdriver::server::start(addr, allow_hosts, Vec::new(), handler, Vec::new())
        .map_err(|e| anyhow!("failed to start WebDriver server on {addr}: {e}"))?;
    tracing::info!(%addr, "servo-fetch WebDriver server listening");
    tracing::warn!(
        "WebDriver sessions honor the process SSRF policy; pass --allow-private-addresses to reach local targets"
    );

    shutdown_signal().await;
    tracing::info!("shutting down WebDriver server");
    // Dropping the `Listener` would block forever joining the warp thread; the
    // background server threads are torn down by process exit instead.
    std::mem::forget(listener);
    Ok(())
}

/// W3C `WebDriver` handler backed by the Servo engine. The `webdriver` dispatcher
/// runs one handler on one thread, so a single active session suffices.
struct ServoWdHandler {
    engine: WebDriverEngine,
    session: Option<SessionId>,
}

impl ServoWdHandler {
    fn new() -> Self {
        Self {
            engine: WebDriverEngine::new(),
            session: None,
        }
    }

    /// The active session, or an `InvalidSessionId` error.
    fn active(&self) -> WebDriverResult<&SessionId> {
        self.session
            .as_ref()
            .ok_or_else(|| WebDriverError::new(ErrorStatus::InvalidSessionId, "no active WebDriver session"))
    }

    /// Run an engine operation against the active session, mapping errors.
    fn with_session<T>(&self, f: impl FnOnce(&WebDriverEngine, &SessionId) -> anyhow::Result<T>) -> WebDriverResult<T> {
        let session = self.active()?;
        to_wd(f(&self.engine, session))
    }

    fn new_session(&mut self) -> WebDriverResult<WebDriverResponse> {
        let id = self
            .engine
            .new_session()
            .map_err(|e| WebDriverError::new(ErrorStatus::SessionNotCreated, e.to_string()))?;
        let response = NewSessionResponse::new(id.as_str().to_string(), session_capabilities());
        self.session = Some(id);
        Ok(WebDriverResponse::NewSession(response))
    }

    fn delete_session(&mut self) -> WebDriverResult<WebDriverResponse> {
        if let Some(session) = self.session.take() {
            to_wd(self.engine.delete_session(&session))?;
        }
        Ok(WebDriverResponse::DeleteSession)
    }

    /// Find elements, retrying until non-empty or the implicit timeout elapses.
    fn locate(&self, locator: &LocatorParameters) -> WebDriverResult<Vec<String>> {
        let session = self.active()?;
        let implicit = to_wd(self.engine.get_timeouts(session))?.implicit;
        let strategy = locator_of(locator.using);
        let value = locator.value.as_str();
        to_wd(find_with_wait(Duration::from_millis(implicit), || {
            self.engine.find_elements(session, strategy, value)
        }))
    }

    fn find_element(&self, locator: &LocatorParameters) -> WebDriverResult<WebDriverResponse> {
        let first = self.locate(locator)?.into_iter().next().ok_or_else(|| {
            WebDriverError::new(
                ErrorStatus::NoSuchElement,
                format!(
                    "no element found for {} \"{}\"",
                    strategy_name(locator.using),
                    locator.value
                ),
            )
        })?;
        Ok(generic(serde_json::to_value(WebElement(first))?))
    }

    fn find_elements(&self, locator: &LocatorParameters) -> WebDriverResult<WebDriverResponse> {
        let elements = self
            .locate(locator)?
            .into_iter()
            .map(|id| serde_json::to_value(WebElement(id)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(generic(Value::Array(elements)))
    }
}

impl WebDriverHandler for ServoWdHandler {
    fn handle_command(
        &mut self,
        _session: &Option<Session>,
        msg: WebDriverMessage<VoidWebDriverExtensionRoute>,
    ) -> WebDriverResult<WebDriverResponse> {
        match msg.command {
            WebDriverCommand::Status => Ok(status_response()),
            WebDriverCommand::NewSession(_) => self.new_session(),
            WebDriverCommand::DeleteSession => self.delete_session(),

            // Navigation.
            WebDriverCommand::Get(p) => self
                .with_session(|e, s| e.navigate(s, &p.url))
                .map(|()| WebDriverResponse::Void),
            WebDriverCommand::GetCurrentUrl => self.with_session(WebDriverEngine::current_url).map(string_value),
            WebDriverCommand::GoBack => self
                .with_session(WebDriverEngine::back)
                .map(|()| WebDriverResponse::Void),
            WebDriverCommand::GoForward => self
                .with_session(WebDriverEngine::forward)
                .map(|()| WebDriverResponse::Void),
            WebDriverCommand::Refresh => self
                .with_session(WebDriverEngine::refresh)
                .map(|()| WebDriverResponse::Void),

            // Document.
            WebDriverCommand::GetTitle => self.with_session(WebDriverEngine::title).map(string_value),
            WebDriverCommand::GetPageSource => self.with_session(WebDriverEngine::page_source).map(string_value),

            // Locators.
            WebDriverCommand::FindElement(loc) => self.find_element(&loc),
            WebDriverCommand::FindElements(loc) => self.find_elements(&loc),

            // Element getters.
            WebDriverCommand::GetElementText(el) => {
                self.with_session(|e, s| e.element_text(s, &el.0)).map(string_value)
            }
            WebDriverCommand::GetElementAttribute(el, name) => self
                .with_session(|e, s| e.element_attribute(s, &el.0, &name))
                .map(opt_string_value),
            WebDriverCommand::GetElementProperty(el, name) => self
                .with_session(|e, s| e.element_property(s, &el.0, &name))
                .map(generic),
            WebDriverCommand::GetCSSValue(el, name) => self
                .with_session(|e, s| e.element_css(s, &el.0, &name))
                .map(string_value),
            WebDriverCommand::GetElementTagName(el) => {
                self.with_session(|e, s| e.element_tag_name(s, &el.0)).map(string_value)
            }
            WebDriverCommand::GetElementRect(el) => self
                .with_session(|e, s| e.element_rect(s, &el.0))
                .map(element_rect_response),
            WebDriverCommand::IsEnabled(el) => self.with_session(|e, s| e.is_enabled(s, &el.0)).map(bool_value),
            WebDriverCommand::IsSelected(el) => self.with_session(|e, s| e.is_selected(s, &el.0)).map(bool_value),
            WebDriverCommand::IsDisplayed(el) => self.with_session(|e, s| e.is_displayed(s, &el.0)).map(bool_value),

            // Interactions.
            WebDriverCommand::ElementClick(el) => self
                .with_session(|e, s| e.click_element(s, &el.0))
                .map(|()| WebDriverResponse::Void),
            WebDriverCommand::ElementClear(el) => self
                .with_session(|e, s| e.clear_element(s, &el.0))
                .map(|()| WebDriverResponse::Void),
            WebDriverCommand::ElementSendKeys(el, p) => self
                .with_session(|e, s| e.send_keys(s, &el.0, &p.text))
                .map(|()| WebDriverResponse::Void),

            // Scripting.
            WebDriverCommand::ExecuteScript(p) => self
                .with_session(|e, s| e.execute_script(s, &p.script, &p.args.clone().unwrap_or_default()))
                .map(generic),
            WebDriverCommand::ExecuteAsyncScript(p) => self
                .with_session(|e, s| e.execute_async_script(s, &p.script, &p.args.clone().unwrap_or_default()))
                .map(generic),

            // Timeouts.
            WebDriverCommand::GetTimeouts => self.with_session(WebDriverEngine::get_timeouts).map(timeouts_response),
            WebDriverCommand::SetTimeouts(p) => self
                .with_session(|e, s| e.set_timeouts(s, p.script, p.page_load, p.implicit))
                .map(|()| WebDriverResponse::Void),

            // Window rect.
            WebDriverCommand::GetWindowRect => self
                .with_session(WebDriverEngine::window_rect)
                .map(window_rect_response),
            WebDriverCommand::SetWindowRect(p) => self
                .with_session(|e, s| e.set_window_rect(s, p.width, p.height))
                .map(window_rect_response),

            // Screenshots.
            WebDriverCommand::TakeScreenshot => self.with_session(WebDriverEngine::screenshot).map(screenshot_value),
            WebDriverCommand::TakeElementScreenshot(el) => self
                .with_session(|e, s| e.element_screenshot(s, &el.0))
                .map(screenshot_value),

            _ => Err(WebDriverError::new(
                ErrorStatus::UnsupportedOperation,
                "command not supported by servo-fetch",
            )),
        }
    }

    fn teardown_session(&mut self, _kind: SessionTeardownKind) {
        if let Some(session) = self.session.take() {
            let _ = self.engine.delete_session(&session);
        }
    }
}

/// Map an engine error to the closest W3C error status, inspecting the message
/// for the `ErrorStatus` names Servo's script thread reports.
fn error_status_of(error: &anyhow::Error) -> ErrorStatus {
    let message = error.to_string();
    let has = |needle: &str| message.contains(needle);
    if has("NoSuchElement") {
        ErrorStatus::NoSuchElement
    } else if has("StaleElement") {
        ErrorStatus::StaleElementReference
    } else if has("ElementClickIntercepted") {
        ErrorStatus::ElementClickIntercepted
    } else if has("ElementNotInteractable") {
        ErrorStatus::ElementNotInteractable
    } else if has("InvalidSelector") {
        ErrorStatus::InvalidSelector
    } else if has("InvalidElementState") {
        ErrorStatus::InvalidElementState
    } else if has("no such WebDriver session") {
        ErrorStatus::InvalidSessionId
    } else if has("async script error") || has("JS eval") || has("JavaScript") {
        ErrorStatus::JavascriptError
    } else if has("timed out") || has("Timeout") {
        ErrorStatus::Timeout
    } else {
        ErrorStatus::UnknownError
    }
}

fn to_wd<T>(result: anyhow::Result<T>) -> WebDriverResult<T> {
    result.map_err(|e| WebDriverError::new(error_status_of(&e), e.to_string()))
}

/// Find elements, retrying until the result is non-empty or `implicit` elapses.
/// Always performs at least one attempt.
fn find_with_wait(
    implicit: Duration,
    mut find: impl FnMut() -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<Vec<String>> {
    let deadline = Instant::now() + implicit;
    loop {
        let found = find()?;
        if !found.is_empty() || Instant::now() >= deadline {
            return Ok(found);
        }
        std::thread::sleep(FIND_POLL);
    }
}

fn locator_of(strategy: LocatorStrategy) -> Locator {
    match strategy {
        LocatorStrategy::CSSSelector => Locator::Css,
        LocatorStrategy::LinkText => Locator::LinkText,
        LocatorStrategy::PartialLinkText => Locator::PartialLinkText,
        LocatorStrategy::TagName => Locator::TagName,
        LocatorStrategy::XPath => Locator::XPath,
    }
}

fn strategy_name(strategy: LocatorStrategy) -> &'static str {
    match strategy {
        LocatorStrategy::CSSSelector => "css selector",
        LocatorStrategy::LinkText => "link text",
        LocatorStrategy::PartialLinkText => "partial link text",
        LocatorStrategy::TagName => "tag name",
        LocatorStrategy::XPath => "xpath",
    }
}

fn session_capabilities() -> Value {
    json!({
        "browserName": "servo",
        "browserVersion": env!("CARGO_PKG_VERSION"),
        "platformName": std::env::consts::OS,
        "acceptInsecureCerts": false,
        "setWindowRect": true,
        "strictFileInteractability": false,
    })
}

fn status_response() -> WebDriverResponse {
    generic(json!({ "ready": true, "message": "servo-fetch WebDriver ready" }))
}

fn generic(value: Value) -> WebDriverResponse {
    WebDriverResponse::Generic(ValueResponse(value))
}

fn string_value(value: String) -> WebDriverResponse {
    generic(Value::String(value))
}

fn opt_string_value(value: Option<String>) -> WebDriverResponse {
    generic(value.map_or(Value::Null, Value::String))
}

fn bool_value(value: bool) -> WebDriverResponse {
    generic(Value::Bool(value))
}

fn screenshot_value(png: Vec<u8>) -> WebDriverResponse {
    generic(Value::String(base64::engine::general_purpose::STANDARD.encode(png)))
}

fn element_rect_response(rect: ElementRect) -> WebDriverResponse {
    WebDriverResponse::ElementRect(ElementRectResponse {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    })
}

fn window_rect_response(rect: WindowRect) -> WebDriverResponse {
    WebDriverResponse::WindowRect(WindowRectResponse {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    })
}

fn timeouts_response(timeouts: Timeouts) -> WebDriverResponse {
    WebDriverResponse::Timeouts(TimeoutsResponse::new(
        timeouts.script,
        timeouts.page_load,
        timeouts.implicit,
    ))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn locator_maps_all_strategies() {
        assert!(matches!(locator_of(LocatorStrategy::CSSSelector), Locator::Css));
        assert!(matches!(locator_of(LocatorStrategy::LinkText), Locator::LinkText));
        assert!(matches!(
            locator_of(LocatorStrategy::PartialLinkText),
            Locator::PartialLinkText
        ));
        assert!(matches!(locator_of(LocatorStrategy::TagName), Locator::TagName));
        assert!(matches!(locator_of(LocatorStrategy::XPath), Locator::XPath));
    }

    #[test]
    fn error_status_recognizes_known_names() {
        assert_eq!(
            error_status_of(&anyhow!("get element text failed: NoSuchElement")),
            ErrorStatus::NoSuchElement
        );
        assert_eq!(
            error_status_of(&anyhow!("element click failed: ElementClickIntercepted")),
            ErrorStatus::ElementClickIntercepted
        );
        assert_eq!(error_status_of(&anyhow!("navigation timed out")), ErrorStatus::Timeout);
        assert_eq!(
            error_status_of(&anyhow!("no such WebDriver session: servo-fetch-9")),
            ErrorStatus::InvalidSessionId
        );
        assert_eq!(
            error_status_of(&anyhow!("something unexpected")),
            ErrorStatus::UnknownError
        );
    }

    #[test]
    fn capabilities_include_browser_name() {
        let caps = session_capabilities();
        assert_eq!(caps["browserName"], "servo");
        assert_eq!(caps["setWindowRect"], true);
    }

    #[test]
    fn find_with_wait_returns_first_non_empty_immediately() {
        let calls = Cell::new(0);
        let result = find_with_wait(Duration::from_secs(5), || {
            calls.set(calls.get() + 1);
            Ok(vec!["el-1".to_string()])
        })
        .unwrap();
        assert_eq!(result, vec!["el-1".to_string()]);
        assert_eq!(calls.get(), 1, "should not retry once a result is found");
    }

    #[test]
    fn find_with_wait_returns_empty_after_one_attempt_with_zero_timeout() {
        let calls = Cell::new(0);
        let result = find_with_wait(Duration::ZERO, || {
            calls.set(calls.get() + 1);
            Ok(Vec::new())
        })
        .unwrap();
        assert!(result.is_empty());
        assert_eq!(calls.get(), 1, "implicit=0 means exactly one attempt");
    }

    #[test]
    fn find_with_wait_retries_until_found() {
        let calls = Cell::new(0);
        let result = find_with_wait(Duration::from_secs(5), || {
            calls.set(calls.get() + 1);
            if calls.get() >= 3 {
                Ok(vec!["el-late".to_string()])
            } else {
                Ok(Vec::new())
            }
        })
        .unwrap();
        assert_eq!(result, vec!["el-late".to_string()]);
        assert!(calls.get() >= 3, "should retry until elements appear");
    }

    #[test]
    fn find_with_wait_propagates_errors() {
        let result = find_with_wait(Duration::from_secs(5), || Err(anyhow!("boom")));
        assert!(result.is_err());
    }
}
