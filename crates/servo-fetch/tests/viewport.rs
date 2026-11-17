//! Custom viewport integration test.

use image::GenericImageView;
use servo_fetch::blocking::fetch;
use servo_fetch::{FetchOptions, NetworkPolicy, Viewport};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "e2e: spawns the Servo engine; Linux CI sees SIGSEGV during destructor cleanup"]
async fn viewport_sizes_layout_and_screenshot() {
    servo_fetch::init(NetworkPolicy::PERMISSIVE).unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            b"<!doctype html><html><body><p>viewport</p></body></html>".as_slice(),
            "text/html; charset=utf-8",
        ))
        .mount(&server)
        .await;
    let url = format!("{}/", server.uri());
    let viewport = Viewport::new(390, 844).unwrap();

    let (layout, screenshot) = tokio::task::spawn_blocking(move || {
        let layout = fetch(&FetchOptions::javascript(&url, "`${innerWidth}x${innerHeight}`").viewport(viewport))
            .expect("javascript fetch")
            .js_result;
        let png = fetch(&FetchOptions::screenshot(&url, false).viewport(viewport))
            .expect("screenshot fetch")
            .screenshot_png()
            .map(<[u8]>::to_vec)
            .expect("screenshot bytes");
        (layout, image::load_from_memory(&png).expect("png").dimensions())
    })
    .await
    .expect("spawn_blocking");

    assert_eq!((layout.as_deref(), screenshot), (Some("390x844"), (390, 844)));
}
