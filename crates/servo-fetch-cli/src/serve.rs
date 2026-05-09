//! HTTP API server — REST endpoints for fetching, rendering, and extracting web content.

mod error;
mod extract;
mod handlers;
mod params;

use std::net::SocketAddr;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

const MAX_BODY_BYTES: usize = 1024 * 1024;

fn router() -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/version", get(handlers::version))
        .route("/v1/fetch", post(handlers::fetch))
        .route("/v1/batch_fetch", post(handlers::batch_fetch))
        .route("/v1/screenshot", post(handlers::screenshot))
        .route("/v1/execute_js", post(handlers::execute_js))
        .route("/v1/crawl", post(handlers::crawl))
        .route("/v1/map", post(handlers::map))
        .layer(
            ServiceBuilder::new()
                .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(TraceLayer::new_for_http())
                .layer(PropagateRequestIdLayer::x_request_id()),
        )
}

pub(crate) async fn run(host: &str, port: u16) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "servo-fetch HTTP API listening");

    axum::serve(listener, router())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
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
    use super::*;

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _;

    async fn send(req: Request<Body>) -> (StatusCode, String, String) {
        let res = router().oneshot(req).await.unwrap();
        let status = res.status();
        let content_type = res
            .headers()
            .get("content-type")
            .map_or("", |v| v.to_str().unwrap_or(""))
            .to_string();
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (status, content_type, String::from_utf8(body.to_vec()).unwrap())
    }

    fn json_post(path: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn health_returns_ok_json() {
        let (status, ct, body) = send(Request::get("/health").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(ct.starts_with("application/json"));
        assert_eq!(body, r#"{"status":"ok"}"#);
    }

    #[tokio::test]
    async fn version_returns_name_and_version() {
        let (status, ct, body) = send(Request::get("/version").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(ct.starts_with("application/json"));
        assert!(body.contains(r#""name":"servo-fetch""#), "got: {body}");
        assert!(body.contains(r#""version":"#), "got: {body}");
    }

    #[tokio::test]
    async fn unknown_path_returns_404() {
        let (status, _, _) = send(Request::get("/does-not-exist").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn wrong_method_returns_405() {
        let (status, _, _) = send(Request::post("/health").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn fetch_file_scheme_rejected_as_json_400() {
        let (status, ct, body) = send(json_post("/v1/fetch", r#"{"url":"file:///etc/passwd"}"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(ct.starts_with("application/json"));
        assert!(body.contains(r#""error""#));
        assert!(body.contains("scheme 'file'"), "got: {body}");
    }

    #[tokio::test]
    async fn fetch_loopback_rejected_as_json_400() {
        let (status, ct, body) = send(json_post("/v1/fetch", r#"{"url":"http://127.0.0.1/"}"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(ct.starts_with("application/json"));
        assert!(body.contains("address not allowed"), "got: {body}");
    }

    #[tokio::test]
    async fn fetch_missing_field_returns_json_error() {
        let (status, ct, body) = send(json_post("/v1/fetch", "{}")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(ct.starts_with("application/json"));
        assert!(body.contains(r#""error""#), "got: {body}");
    }

    #[tokio::test]
    async fn fetch_malformed_json_returns_json_error() {
        let (status, ct, body) = send(json_post("/v1/fetch", "not json")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(ct.starts_with("application/json"));
        assert!(body.contains(r#""error""#), "got: {body}");
    }

    #[tokio::test]
    async fn fetch_wrong_content_type_returns_json_error() {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/fetch")
            .header("content-type", "text/plain")
            .body(Body::from("{}"))
            .unwrap();
        let (status, ct, body) = send(req).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert!(ct.starts_with("application/json"));
        assert!(body.contains(r#""error""#), "got: {body}");
    }

    #[tokio::test]
    async fn fetch_body_too_large_returns_json_error() {
        let big = format!(
            r#"{{"url":"https://example.com","selector":"{}"}}"#,
            "a".repeat(MAX_BODY_BYTES + 1)
        );
        let (status, ct, body) = send(json_post("/v1/fetch", &big)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(ct.starts_with("application/json"));
        assert!(body.contains(r#""error""#), "got: {body}");
    }

    #[tokio::test]
    async fn screenshot_file_scheme_rejected_as_json_400() {
        let (status, ct, body) = send(json_post("/v1/screenshot", r#"{"url":"file:///"}"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(ct.starts_with("application/json"));
        assert!(body.contains("scheme 'file'"), "got: {body}");
    }

    #[tokio::test]
    async fn batch_fetch_empty_urls_returns_400_json() {
        let (status, ct, body) = send(json_post("/v1/batch_fetch", r#"{"urls":[]}"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(ct.starts_with("application/json"));
        assert!(body.contains("must not be empty"), "got: {body}");
    }

    #[tokio::test]
    async fn request_id_is_auto_generated() {
        let res = router()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let rid = res.headers().get("x-request-id").expect("x-request-id header");
        assert!(!rid.is_empty());
    }

    #[tokio::test]
    async fn request_id_is_propagated_when_supplied() {
        let req = Request::builder()
            .method("GET")
            .uri("/health")
            .header("x-request-id", "my-trace-abc-123")
            .body(Body::empty())
            .unwrap();
        let res = router().oneshot(req).await.unwrap();
        assert_eq!(res.headers().get("x-request-id").unwrap(), "my-trace-abc-123");
    }
}
