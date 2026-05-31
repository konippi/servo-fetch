//! Visibility-aware extraction integration tests.

use servo_fetch::blocking::fetch;
use servo_fetch::{FetchOptions, NetworkPolicy, VisibilityPolicy};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<title>Visibility E2E</title>
<style>
.opacity-0 { opacity: 0; }
.clipped { clip-path: inset(100%); position: absolute; }
.cv-hidden { content-visibility: hidden; }
.text-indent-off { text-indent: -9999px; }
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0, 0, 0, 0); }
</style>
</head>
<body>
<nav class="site-nav"><p>NAV-MARKER</p></nav>
<header role="banner"><p>BANNER-MARKER</p></header>
<main>
  <article>
    <h1>Article Title</h1>
    <p>VISIBLE-MARKER and some additional words to satisfy Readability heuristics for content density.</p>
    <p>VISIBLE-MARKER-2 second paragraph with enough text to be considered article content by the extractor.</p>
    <p class="opacity-0">OPACITY-ZERO-MARKER</p>
    <p class="clipped">CLIPPED-MARKER</p>
    <p class="cv-hidden">CV-HIDDEN-MARKER</p>
    <p class="text-indent-off">TEXT-INDENT-MARKER</p>
    <p class="sr-only">SR-ONLY-MARKER</p>
    <p hidden>HIDDEN-ATTR-MARKER</p>
    <p aria-hidden="true">ARIA-HIDDEN-MARKER</p>
  </article>
</main>
<footer role="contentinfo"><p>FOOTER-MARKER</p></footer>
<div role="dialog" aria-modal="true"><p>MODAL-MARKER</p></div>
</body>
</html>"#;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "e2e: spawns the Servo engine; Linux CI sees SIGSEGV during destructor cleanup"]
async fn moderate_policy_filters_hidden_content() {
    servo_fetch::init(NetworkPolicy::PERMISSIVE);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(HTML.as_bytes(), "text/html; charset=utf-8"))
        .mount(&server)
        .await;
    let url = format!("{}/", server.uri());

    let md = tokio::task::spawn_blocking(move || {
        fetch(&FetchOptions::new(&url).visibility(VisibilityPolicy::moderate()))
            .expect("fetch")
            .markdown()
            .expect("markdown")
    })
    .await
    .expect("spawn_blocking");

    assert!(md.contains("VISIBLE-MARKER"), "main content missing:\n{md}");
    assert!(md.contains("SR-ONLY-MARKER"), "moderate must keep sr-only:\n{md}");

    for marker in [
        "HIDDEN-ATTR-MARKER",
        "ARIA-HIDDEN-MARKER",
        "MODAL-MARKER",
        "FOOTER-MARKER",
        "OPACITY-ZERO-MARKER",
        "CLIPPED-MARKER",
        "TEXT-INDENT-MARKER",
    ] {
        assert!(!md.contains(marker), "{marker} leaked into markdown:\n{md}");
    }

    // Tracked limitations: a11y tree fires only on dynamic updates.
    for marker in ["NAV-MARKER", "BANNER-MARKER", "CV-HIDDEN-MARKER"] {
        assert!(md.contains(marker), "{marker} now stripped — gap closed?\n{md}");
    }
}
