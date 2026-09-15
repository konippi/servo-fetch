//! CLI integration tests.

use std::fs;
use std::future::Future;
use std::str::from_utf8;
#[cfg(unix)]
use std::{
    io::Read as _,
    os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
    process::Stdio,
    time::{Duration, Instant},
};

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::mock_page;

fn servo_fetch() -> Command {
    Command::cargo_bin("servo-fetch").expect("binary exists")
}

fn block_on<F: Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build current-thread runtime")
        .block_on(f)
}

#[test]
fn no_args_shows_error() {
    servo_fetch()
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL is required"));
}

#[test]
fn invalid_url_shows_error() {
    servo_fetch()
        .arg("not-a-url")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid URL"));
}

#[test]
fn file_scheme_rejected() {
    servo_fetch()
        .arg("file:///etc/passwd")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not allowed"));
}

#[test]
fn javascript_scheme_rejected() {
    servo_fetch()
        .arg("javascript:alert(1)")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not allowed"));
}

#[test]
fn version_flag() {
    servo_fetch()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("servo-fetch"));
}

#[test]
fn help_flag() {
    servo_fetch()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("browser engine in a binary"))
        .stdout(predicate::str::contains("__worker").not());
}

#[test]
fn internal_worker_dispatches_before_clap_and_logging() {
    #[derive(serde::Deserialize)]
    struct WorkerProtocolInfo {
        magic: [u8; 8],
        package_version: String,
    }
    let output = servo_fetch().arg("__worker").assert().success().get_output().clone();
    assert!(output.stderr.is_empty());
    let length = usize::try_from(u32::from_be_bytes(output.stdout[..4].try_into().unwrap())).unwrap();
    assert_eq!(output.stdout.len(), length + 4);
    let info: WorkerProtocolInfo = postcard::from_bytes(&output.stdout[4..]).unwrap();
    assert_eq!(info.magic, *b"SFETCHW\0");
    assert_eq!(info.package_version, env!("CARGO_PKG_VERSION"));
}

#[cfg(unix)]
#[test]
#[allow(unsafe_code)]
fn internal_worker_exits_when_parent_lifeline_closes() {
    let mut descriptors = [-1; 2];
    assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
    let read_end = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
    let write_end = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
    let flags = unsafe { libc::fcntl(write_end.as_raw_fd(), libc::F_GETFD) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(write_end.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) },
        0
    );

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_servo-fetch"))
        .arg("__worker")
        .env(
            "SERVO_FETCH_INTERNAL_PARENT_LIFELINE_FD",
            read_end.as_raw_fd().to_string(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(read_end);
    let _stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut prefix = [0; 4];
    stdout.read_exact(&mut prefix).unwrap();
    let mut info = vec![0; usize::try_from(u32::from_be_bytes(prefix)).unwrap()];
    stdout.read_exact(&mut info).unwrap();
    drop(write_end);

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "worker survived parent lifeline closure");
        std::thread::sleep(Duration::from_millis(10));
    }
}

const TIMEOUT: &str = "--timeout=30";

#[test]
#[ignore = "e2e: requires Servo engine"]
fn default_produces_markdown() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>Test</title></head><body><h1>Hello Servo</h1></body></html>",
            ))
            .mount(&s)
            .await;
        servo_fetch()
            .args(["--allow-private-addresses", TIMEOUT, &s.uri()])
            .assert()
            .success()
            .stdout(predicate::str::contains("Hello Servo"));
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn csp_sandbox_pipeline_crash_fails_fast() {
    block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-security-policy", "sandbox")
                    .set_body_raw(
                        b"<!doctype html><html><body>Sandboxed</body></html>",
                        "text/html; charset=utf-8",
                    ),
            )
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let jar = dir.path().join("cookies.txt");
        let started = Instant::now();
        let output = servo_fetch()
            .args([
                "--allow-private-addresses",
                "--cookie-jar",
                jar.to_str().unwrap(),
                "--timeout",
                "30",
                &server.uri(),
            ])
            .output()
            .expect("run servo-fetch");
        let elapsed = started.elapsed();
        let stderr = from_utf8(&output.stderr).expect("stderr is UTF-8");
        let stdout = from_utf8(&output.stdout).expect("stdout is UTF-8");

        assert_eq!(output.status.code(), Some(70), "stdout: {stdout}; stderr: {stderr}");
        assert!(
            elapsed < Duration::from_secs(10),
            "elapsed: {elapsed:?}; stdout: {stdout}; stderr: {stderr}"
        );
        assert!(stderr.contains("page crashed"), "stdout: {stdout}; stderr: {stderr}");
        assert!(!stderr.contains("timed out"), "stderr: {stderr}");
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn slow_first_byte_is_not_mistaken_for_a_crash() {
    block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                mock_page("<!doctype html><html><body><h1>Late</h1></body></html>")
                    .set_delay(Duration::from_millis(2500)),
            )
            .mount(&server)
            .await;

        servo_fetch()
            .args(["--allow-private-addresses", TIMEOUT, &server.uri()])
            .assert()
            .success()
            .stdout(predicate::str::contains("Late"));
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn cookie_jar_captures_http_and_javascript_cookies_without_json_leakage() {
    block_on(async {
        const HTTP_SECRET: &str = "HTTP_ONLY_SECRET_395";
        const DOMAIN_SECRET: &str = "DOMAIN_SCOPE_SECRET_395";
        const JS_SECRET: &str = "JAVASCRIPT_SECRET_395";

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                mock_page(concat!(
                    "<!doctype html><html><body>cookie test",
                    "<script>document.cookie='ui=' + 'JAVASCRIPT_' + 'SECRET_395' + '; Path=/'</script>",
                    "</body></html>",
                ))
                .insert_header("set-cookie", format!("sid={HTTP_SECRET}; HttpOnly; Path=/"))
                .append_header(
                    "set-cookie",
                    format!("domain={DOMAIN_SECRET}; Domain=www.localhost; Path=/"),
                ),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/verify"))
            .respond_with(mock_page("<!doctype html><html><body>verify</body></html>"))
            .mount(&server)
            .await;
        let url = format!("http://www.localhost:{}", server.address().port());
        let dir = tempfile::tempdir().expect("tempdir");
        let jar = dir.path().join("cookies.txt");
        let assertion = servo_fetch()
            .args([
                "--format",
                "json",
                "--cookie-jar",
                jar.to_str().unwrap(),
                "--allow-private-addresses",
                TIMEOUT,
                &url,
            ])
            .assert()
            .success();
        let output = assertion.get_output();
        let contents = fs::read_to_string(&jar).expect("cookie jar written");
        let mut rows = contents.lines().filter(|line| line.contains('\t')).collect::<Vec<_>>();
        rows.sort_unstable();
        assert_eq!(
            rows,
            [
                format!("#HttpOnly_www.localhost\tFALSE\t/\tFALSE\t0\tsid\t{HTTP_SECRET}"),
                format!(".www.localhost\tTRUE\t/\tFALSE\t0\tdomain\t{DOMAIN_SECRET}"),
                format!("www.localhost\tFALSE\t/\tFALSE\t0\tui\t{JS_SECRET}"),
            ]
        );
        let child_url = format!("http://child.www.localhost:{}/verify", server.address().port());
        servo_fetch()
            .args([
                "--cookies",
                jar.to_str().unwrap(),
                "--js",
                "document.cookie",
                "--allow-private-addresses",
                TIMEOUT,
                &child_url,
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains(format!("domain={DOMAIN_SECRET}")));
        for stream in [&output.stdout, &output.stderr] {
            let text = from_utf8(stream).unwrap();
            assert!(
                !text.contains(HTTP_SECRET) && !text.contains(DOMAIN_SECRET) && !text.contains(JS_SECRET),
                "{text}"
            );
        }
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn json_produces_valid_json() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>JSON Test</title></head><body>content</body></html>",
            ))
            .mount(&s)
            .await;
        let output = servo_fetch()
            .args(["--format", "json", "--allow-private-addresses", TIMEOUT, &s.uri()])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let parsed: Value = serde_json::from_slice(&output).expect("valid JSON");
        assert!(parsed.get("title").is_some());
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn json_uses_redirected_document_url_for_output_and_links() {
    block_on(async {
        let start = MockServer::start().await;
        let final_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/start"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", format!("{}/final", final_server.uri())))
            .mount(&start)
            .await;
        Mock::given(method("GET"))
            .and(path("/final"))
            .respond_with(mock_page(
                "<!doctype html><html><body><main><a href=\"rel\">Relative link</a></main></body></html>",
            ))
            .mount(&final_server)
            .await;

        let output = servo_fetch()
            .args([
                "--format",
                "json",
                "--allow-private-addresses",
                TIMEOUT,
                &format!("{}/start", start.uri()),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let parsed: Value = serde_json::from_slice(&output).expect("valid JSON");
        assert_eq!(parsed["url"], format!("{}/final", final_server.uri()));
        assert!(
            parsed["textContent"]
                .as_str()
                .is_some_and(|markdown| markdown.contains(&format!("{}/rel", final_server.uri())))
        );
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn json_uses_push_state_document_url() {
    block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/start"))
            .respond_with(mock_page(
                "<!doctype html><html><body><main>Push state</main><script>history.pushState({}, '', '/pushed')</script></body></html>",
            ))
            .mount(&server)
            .await;

        let output = servo_fetch()
            .args([
                "--format",
                "json",
                "--allow-private-addresses",
                TIMEOUT,
                &format!("{}/start", server.uri()),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let parsed: Value = serde_json::from_slice(&output).expect("valid JSON");
        assert_eq!(parsed["url"], format!("{}/pushed", server.uri()));
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn js_eval_returns_result() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>JS Eval</title></head><body></body></html>",
            ))
            .mount(&s)
            .await;
        servo_fetch()
            .args(["--js", "document.title", "--allow-private-addresses", TIMEOUT, &s.uri()])
            .assert()
            .success();
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn format_png_creates_file() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page("<html><body><h1>Screenshot</h1></body></html>"))
            .mount(&s)
            .await;
        let dir = std::env::temp_dir().join("servo-fetch-e2e");
        fs::create_dir_all(&dir).ok();
        let file = dir.join("test.png");
        servo_fetch()
            .args([
                "--format",
                "png",
                "-o",
                file.to_str().unwrap(),
                "--allow-private-addresses",
                TIMEOUT,
                &s.uri(),
            ])
            .assert()
            .success();
        assert!(file.exists());
        assert!(file.metadata().unwrap().len() > 0);
        fs::remove_file(&file).ok();
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn full_page_screenshot_stabilizes_resize_triggered_content() {
    block_on(async {
        let server = MockServer::start().await;
        let html = r#"<!doctype html>
<html>
<head>
<style>
  html, body { margin: 0; }
  .spacer { height: 1200px; }
  #target { height: 100px; opacity: 0; }
  #target.revealed { height: 1000px; opacity: 1; }
  #target img { display: block; width: 120px; height: 120px; margin-top: 800px; }
  .tail { height: 200px; }
</style>
</head>
<body>
  <div class="spacer"></div>
  <div id="target"></div>
  <div class="tail"></div>
  <script>
    const target = document.querySelector('#target');
    new IntersectionObserver(entries => {
      if (!entries.some(entry => entry.isIntersecting)) return;
      target.classList.add('revealed');
      const image = document.createElement('img');
      image.src = '/purple.svg';
      target.appendChild(image);
    }).observe(target);
  </script>
</body>
</html>"#;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(html))
            .mount(&server)
            .await;
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120"><rect width="120" height="120" fill="#9333ea"/></svg>"##;
        Mock::given(method("GET"))
            .and(path("/purple.svg"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(svg.as_bytes(), "image/svg+xml"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("full-page-io.png");
        servo_fetch()
            .args([
                "--format",
                "png",
                "--full-page",
                "-o",
                file.to_str().unwrap(),
                "--allow-private-addresses",
                TIMEOUT,
                &server.uri(),
            ])
            .assert()
            .success();

        let screenshot = image::open(&file).expect("decode PNG").to_rgb8();
        assert!(screenshot.height() >= 2_300, "dynamic page growth was clipped");
        let purple = screenshot
            .pixels()
            .filter(|pixel| pixel.0 == [0x93, 0x33, 0xea])
            .count();
        assert!(purple > 5_000, "IntersectionObserver image was not painted");
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn crawl_produces_ndjson() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>Crawl</title></head><body><p>Root</p></body></html>",
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let output = servo_fetch()
            .args([
                "crawl",
                &s.uri(),
                "--format",
                "json",
                "--limit",
                "1",
                "--timeout",
                "30",
                "--allow-private-addresses",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let lines: Vec<&str> = from_utf8(&output).unwrap().lines().collect();
        assert!(lines.len() >= 2, "expected page + stats record, got: {lines:?}");
        let page: Value = serde_json::from_str(lines[0]).expect("valid NDJSON");
        assert_eq!(page["type"], "page");
        assert!(page["fetchedAt"].is_string(), "fetchedAt must be present");
        let stats: Value = serde_json::from_str(lines.last().unwrap()).expect("valid stats NDJSON");
        assert_eq!(stats["type"], "stats");
        assert!(stats["crawled"].as_u64().is_some_and(|n| n >= 1));
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn crawl_selector_scopes_content() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>T</title></head><body><h1>Kept</h1><p>Dropped paragraph</p></body></html>",
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let output = servo_fetch()
            .args([
                "crawl",
                &s.uri(),
                "--selector",
                "h1",
                "--limit",
                "1",
                "--max-depth",
                "0",
                "--timeout",
                "30",
                "--allow-private-addresses",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let text = from_utf8(&output).unwrap();
        assert!(text.contains("Kept"), "selector should keep h1 text, got: {text}");
        assert!(
            !text.contains("Dropped paragraph"),
            "selector should scope away from <p>, got: {text}"
        );
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn crawl_json_embeds_structured_content() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>JSONCrawl</title></head><body><article>Structured body content for extraction.</article></body></html>",
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let output = servo_fetch()
            .args([
                "crawl",
                &s.uri(),
                "--format",
                "json",
                "--limit",
                "1",
                "--max-depth",
                "0",
                "--timeout",
                "30",
                "--allow-private-addresses",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let line = from_utf8(&output)
            .unwrap()
            .lines()
            .next()
            .expect("expected NDJSON line");
        let outer: Value = serde_json::from_str(line).expect("outer NDJSON must parse");
        let content_str = outer["content"].as_str().expect("content must be a string");
        let inner: Value = serde_json::from_str(content_str).expect("content must be structured JSON, not markdown");
        assert!(
            inner.get("title").is_some(),
            "inner JSON should have a 'title' field, got: {content_str}"
        );
    });
}

#[test]
fn crawl_rejects_private_ip() {
    servo_fetch()
        .args(["crawl", "http://127.0.0.1/"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not allowed"));
}

#[test]
fn crawl_rejects_file_scheme() {
    servo_fetch()
        .args(["crawl", "file:///etc/passwd"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not allowed"));
}

#[test]
fn crawl_rejects_invalid_url() {
    servo_fetch()
        .args(["crawl", "not-a-url"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid URL"));
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn output_writes_single_file() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>OutputFile</title></head><body><h1>Direct</h1></body></html>",
            ))
            .mount(&s)
            .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("page.md");
        servo_fetch()
            .args([
                "-o",
                file.to_str().unwrap(),
                "--allow-private-addresses",
                TIMEOUT,
                &s.uri(),
            ])
            .assert()
            .success();
        let body = fs::read_to_string(&file).expect("file written");
        assert!(body.contains("Direct"), "got: {body}");
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn output_dir_writes_single_file() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>OutputDir</title></head><body><h1>Saved</h1></body></html>",
            ))
            .mount(&s)
            .await;
        let dir = tempfile::tempdir().expect("tempdir");
        servo_fetch()
            .args([
                "--output-dir",
                dir.path().to_str().unwrap(),
                "--allow-private-addresses",
                TIMEOUT,
                &s.uri(),
            ])
            .assert()
            .success();
        let entries: Vec<_> = fs::read_dir(dir.path())
            .expect("dir exists")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .collect();
        assert_eq!(entries.len(), 1, "expected exactly 1 .md file");
        let body = fs::read_to_string(entries[0].path()).unwrap();
        assert!(body.contains("Saved"), "got: {body}");
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn crawl_output_dir_writes_per_page_files() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>CrawlDir</title></head><body><p>One page</p></body></html>",
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let output = servo_fetch()
            .args([
                "crawl",
                &s.uri(),
                "--format",
                "json",
                "--output-dir",
                dir.path().to_str().unwrap(),
                "--limit",
                "1",
                "--max-depth",
                "0",
                "--timeout",
                "30",
                "--allow-private-addresses",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert!(output.is_empty(), "stdout should be silent in --output-dir mode");
        let entries: Vec<_> = fs::read_dir(dir.path())
            .expect("dir exists")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .collect();
        assert_eq!(entries.len(), 1, "expected exactly 1 .json file");
        let line = fs::read_to_string(entries[0].path()).unwrap();
        let record: Value = serde_json::from_str(line.trim()).expect("valid JSON");
        assert_eq!(record["type"], "page");
    });
}

#[test]
fn crawl_help_shows_options() {
    servo_fetch()
        .args(["crawl", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("--max-depth"))
        .stdout(predicate::str::contains("--include"))
        .stdout(predicate::str::contains("--exclude"))
        .stdout(predicate::str::contains("--output-dir"));
}

#[test]
fn fetch_help_shows_output_dir() {
    servo_fetch()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--output-dir"));
}

#[test]
fn mcp_help_shows_options() {
    servo_fetch()
        .args(["mcp", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MCP"))
        .stdout(predicate::str::contains("--port"));
}
