//! Strong logical browser-session isolation, capacity, and cancellation E2E tests.

use std::sync::Once;
use std::time::Duration;

use servo_fetch::{
    BrowserSessionConfig, FetchOptions, NetworkPolicy, SessionBroker, SessionBrokerConfig, WorkerCommand,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static INIT: Once = Once::new();

fn broker(max_sessions: usize, queue_capacity: usize) -> SessionBroker {
    INIT.call_once(|| servo_fetch::init(NetworkPolicy::PERMISSIVE));
    let config = SessionBrokerConfig::default()
        .max_sessions(max_sessions)
        .queue_capacity(queue_capacity)
        .prewarm(0)
        .acquire_timeout(Duration::from_secs(2))
        .worker_command(WorkerCommand::new(env!("CARGO_BIN_EXE_servo-fetch")).arg("__worker"));
    SessionBroker::new(config).expect("broker starts")
}

async fn page(server: &MockServer, path_name: &str, delay: Option<Duration>) {
    let mut response = ResponseTemplate::new(200).set_body_raw(
        b"<!doctype html><html><body>session test</body></html>".to_vec(),
        "text/html; charset=utf-8",
    );
    if let Some(delay) = delay {
        response = response.set_delay(delay);
    }
    Mock::given(method("GET"))
        .and(path(path_name))
        .respond_with(response)
        .mount(server)
        .await;
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn logical_sessions_isolate_cookie_state() {
    let server = MockServer::start().await;
    page(&server, "/", None).await;
    let broker = broker(2, 2);

    let mut first = broker.session(BrowserSessionConfig::new()).await.unwrap();
    let url = format!("{}/", server.uri());
    let set = first
        .fetch(&FetchOptions::javascript(
            &url,
            "window.isolationMarker = 'page-only'; document.cookie = 'isolation_marker=first; path=/'; document.cookie",
        ))
        .await
        .unwrap();
    assert!(set.js_result.unwrap_or_default().contains("isolation_marker=first"));

    let persisted = first
        .fetch(&FetchOptions::javascript(
            &url,
            "typeof window.isolationMarker + '|' + document.cookie",
        ))
        .await
        .unwrap();
    let persisted = persisted.js_result.unwrap_or_default();
    assert!(persisted.starts_with("undefined|"));
    assert!(persisted.contains("isolation_marker=first"));
    first.close().await.unwrap();

    let mut second = broker.session(BrowserSessionConfig::new()).await.unwrap();
    let isolated = second
        .fetch(&FetchOptions::javascript(&url, "document.cookie"))
        .await
        .unwrap();
    assert!(!isolated.js_result.unwrap_or_default().contains("isolation_marker"));
    second.close().await.unwrap();
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn cancelling_fetch_kills_worker_and_releases_capacity() {
    let server = MockServer::start().await;
    page(&server, "/slow", Some(Duration::from_secs(10))).await;
    let broker = broker(1, 1);
    let url = format!("{}/slow", server.uri());
    let task_broker = broker.clone();

    let task = tokio::spawn(async move {
        let mut session = task_broker.session(BrowserSessionConfig::new()).await.unwrap();
        session
            .fetch(&FetchOptions::new(&url).timeout(Duration::from_secs(30)))
            .await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    task.abort();
    let _ = task.await;

    let replacement = tokio::time::timeout(Duration::from_secs(5), broker.session(BrowserSessionConfig::new()))
        .await
        .expect("cancelled worker should release its broker permit")
        .expect("replacement session starts");
    replacement.close().await.unwrap();
}
