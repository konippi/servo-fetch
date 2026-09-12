//! Strong logical browser-session isolation, capacity, and cancellation E2E tests.

use std::sync::Once;
use std::time::Duration;

use servo_fetch::{
    BrowserSessionConfig, FetchOptions, NetworkPolicy, SessionBroker, SessionBrokerConfig, SessionCancellation,
    WorkerCommand,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

mod common;
use common::{mock_page, slow_page};

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

async fn page(server: &MockServer, path_name: &str) {
    Mock::given(method("GET"))
        .and(path(path_name))
        .respond_with(mock_page("<!doctype html><html><body>session test</body></html>"))
        .mount(server)
        .await;
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn logical_sessions_isolate_cookie_state() {
    let server = MockServer::start().await;
    page(&server, "/").await;
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
    let (mut arrivals, slow) = slow_page("<!doctype html><html><body>slow</body></html>", Duration::from_secs(10));
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(slow)
        .mount(&server)
        .await;
    let broker = broker(1, 1);
    let url = format!("{}/slow", server.uri());
    let task_broker = broker.clone();

    let task = tokio::spawn(async move {
        let mut session = task_broker.session(BrowserSessionConfig::new()).await.unwrap();
        session
            .fetch(&FetchOptions::new(&url).timeout(Duration::from_secs(30)))
            .await
    });
    tokio::time::timeout(Duration::from_secs(15), arrivals.recv())
        .await
        .expect("the worker navigates to the slow page before cancellation");
    task.abort();
    let _ = task.await;

    let replacement = tokio::time::timeout(Duration::from_secs(5), broker.session(BrowserSessionConfig::new()))
        .await
        .expect("cancelled worker should release its broker permit")
        .expect("replacement session starts");
    replacement.close().await.unwrap();
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn cancelling_a_pdf_probe_closes_the_socket_promptly() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind PDF fixture");
    let address = listener.local_addr().expect("PDF fixture address");
    let (partial_tx, partial_rx) = oneshot::channel();
    let (closed_tx, closed_rx) = oneshot::channel();
    let fixture = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept PDF request");
        let mut byte = [0_u8; 1];
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await.expect("read PDF request");
            request.push(byte[0]);
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/pdf\r\ncontent-length: 1048576\r\n\r\n%PDF-1.4\n")
            .await
            .expect("write partial PDF response");
        socket.flush().await.expect("flush partial PDF response");
        partial_tx.send(()).expect("report partial PDF body");
        closed_tx
            .send(socket.read(&mut byte).await)
            .expect("report client disconnect");
    });

    let broker = broker(1, 1);
    let cancellation = SessionCancellation::new();
    let mut session = broker
        .session_with_cancellation(BrowserSessionConfig::new(), &cancellation)
        .await
        .expect("session starts");
    let fetch = tokio::spawn(async move {
        session
            .fetch(&FetchOptions::new(&format!("http://{address}/slow.pdf")).timeout(Duration::from_secs(30)))
            .await
    });
    partial_rx.await.expect("probe reaches the partial PDF body");
    cancellation.cancel();

    let outcome = tokio::time::timeout(Duration::from_secs(1), fetch)
        .await
        .expect("cancelled PDF fetch returns promptly")
        .expect("fetch task completes");
    assert!(
        matches!(outcome, Err(servo_fetch::Error::SessionCancelled)),
        "{outcome:?}"
    );
    match closed_rx.await.expect("fixture reports client disconnect") {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
            ) => {}
        other => panic!("cancelled probe must close its socket, observed {other:?}"),
    }
    fixture.await.expect("PDF fixture completes");
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn cancelled_session_rejects_pdf_fetches_without_touching_the_network() {
    let broker = broker(1, 1);
    let mut session = broker
        .session(BrowserSessionConfig::new())
        .await
        .expect("session starts");
    session.cancel();
    let result = session.fetch(&FetchOptions::new("http://127.0.0.1:9/closed.pdf")).await;
    assert!(
        result.is_err(),
        "a cancelled session must not probe PDFs on the host: {result:?}"
    );
}
