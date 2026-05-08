//! URL discovery via sitemap parsing — no rendering required.

use std::collections::{HashSet, VecDeque};
use std::io::Read as _;
use std::time::{Duration, Instant};

use url::Url;

use crate::bridge;
use crate::net;
use crate::robots::{RobotsPolicy, RobotsRules};
use crate::scope::{is_same_site, matches_scope, normalize_url};

const MAP_SITEMAP_MAX_BYTES: u64 = 50 * 1024 * 1024;
const MAP_SITEMAP_MAX_DECOMPRESSED: u64 = 10 * 1024 * 1024;
const MAP_GZIP_MAX_RATIO: u64 = 100;
const MAP_HTML_MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAP_MAX_REDIRECTS: u8 = 5;
const MAP_MAX_SITEMAPS: usize = 200;
const MAP_MAX_INDEX_DEPTH: u8 = 5;
const MAP_MIN_FETCH_INTERVAL: Duration = Duration::from_millis(500);
const MAP_URL_MAX_LEN: usize = 2048;
const HTML_SNIFF_LEN: usize = 100;

/// Options for URL discovery.
pub(crate) struct MapConfig {
    pub seed: Url,
    pub limit: usize,
    pub include: Option<globset::GlobSet>,
    pub exclude: Option<globset::GlobSet>,
    pub user_agent: Option<String>,
    pub timeout: Duration,
    pub no_fallback: bool,
}

/// A discovered URL with optional metadata.
#[derive(serde::Serialize)]
pub(crate) struct MapEntry {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lastmod: Option<String>,
}

/// Run URL discovery for a site.
pub(crate) async fn run(opts: &MapConfig, mut on_url: impl FnMut(&MapEntry)) {
    let ua = opts
        .user_agent
        .as_deref()
        .unwrap_or_else(|| bridge::default_user_agent());
    let agent = build_agent(ua, opts.timeout);

    let robots = {
        let seed = opts.seed.clone();
        let user_agent = opts.user_agent.clone();
        let timeout = opts.timeout;
        tokio::task::spawn_blocking(move || RobotsRules::fetch(&seed, user_agent.as_deref(), timeout))
            .await
            .unwrap_or(RobotsPolicy::Unreachable)
    };

    let mut visited = HashSet::new();
    let mut count = 0;
    let mut last_fetch = Instant::now()
        .checked_sub(MAP_MIN_FETCH_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut sitemap_queue: VecDeque<(Url, u8)> = discover_sitemaps(&robots, &opts.seed)
        .into_iter()
        .map(|u| (u, 0))
        .collect();
    let mut sitemaps_fetched = 0;

    while let Some((sitemap_url, depth)) = sitemap_queue.pop_front() {
        if sitemaps_fetched >= MAP_MAX_SITEMAPS || count >= opts.limit {
            break;
        }
        if depth > MAP_MAX_INDEX_DEPTH
            || !is_same_site(&opts.seed, &sitemap_url)
            || net::validate_url(sitemap_url.as_str()).is_err()
        {
            continue;
        }

        throttle(&mut last_fetch).await;
        sitemaps_fetched += 1;

        let body = {
            let agent = agent.clone();
            tokio::task::spawn_blocking({
                let seed = opts.seed.clone();
                move || fetch_sitemap(&agent, &sitemap_url, &seed)
            })
            .await
            .ok()
            .flatten()
        };
        let Some(body) = body else { continue };

        for entry in parse_sitemap(&body) {
            match entry {
                SitemapEntry::Url { loc, lastmod } => {
                    if count >= opts.limit {
                        break;
                    }
                    if let Some(e) = validate_entry(&loc, lastmod, &opts.seed, &robots, opts, &mut visited) {
                        on_url(&e);
                        count += 1;
                    }
                }
                SitemapEntry::Sitemap { loc } => {
                    if let Ok(url) = Url::parse(&loc) {
                        sitemap_queue.push_back((url, depth + 1));
                    }
                }
            }
        }
    }

    if count == 0 && !opts.no_fallback {
        throttle(&mut last_fetch).await;
        let html = {
            let agent = agent.clone();
            let seed = opts.seed.clone();
            tokio::task::spawn_blocking(move || fetch_html(&agent, &seed))
                .await
                .ok()
                .flatten()
        };
        if let Some(html) = html {
            for link in extract_links(&html, &opts.seed) {
                if count >= opts.limit {
                    break;
                }
                if let Some(e) = validate_entry(link.as_str(), None, &opts.seed, &robots, opts, &mut visited) {
                    on_url(&e);
                    count += 1;
                }
            }
        }
    }
}

fn discover_sitemaps(robots: &RobotsPolicy, seed: &Url) -> Vec<Url> {
    let mut urls = Vec::new();
    if let RobotsPolicy::Rules(rules) = robots {
        urls.extend(rules.sitemaps.iter().cloned());
    }
    if let Ok(default) = seed.join("/sitemap.xml") {
        if !urls.contains(&default) {
            urls.push(default);
        }
    }
    urls
}

fn build_agent(ua: &str, timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .user_agent(ua)
            .build(),
    )
}

fn fetch_following_redirects(agent: &ureq::Agent, url: &Url, seed: &Url) -> Option<ureq::http::Response<ureq::Body>> {
    let mut current = url.clone();
    for _ in 0..MAP_MAX_REDIRECTS {
        let resp = agent.get(current.as_str()).call().ok()?;
        let status = resp.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let location = resp.headers().get("location")?.to_str().ok()?;
            let next = current.join(location).ok()?;
            if net::validate_url(next.as_str()).is_err() || !is_same_site(seed, &next) {
                return None;
            }
            current = next;
            continue;
        }
        if status >= 400 {
            return None;
        }
        return Some(resp);
    }
    None
}

fn fetch_sitemap(agent: &ureq::Agent, url: &Url, seed: &Url) -> Option<String> {
    let resp = fetch_following_redirects(agent, url, seed)?;
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let is_gzip = url
        .path()
        .rsplit('/')
        .next()
        .and_then(|seg| std::path::Path::new(seg).extension())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gz"))
        || content_type.contains("gzip")
        || resp
            .headers()
            .get("content-encoding")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("gzip"));

    if is_gzip {
        let bytes = resp
            .into_body()
            .with_config()
            .limit(MAP_SITEMAP_MAX_BYTES)
            .read_to_vec()
            .ok()?;
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .take(MAP_SITEMAP_MAX_DECOMPRESSED)
            .read_to_end(&mut decoded)
            .ok()?;
        if decoded.len() as u64 > bytes.len() as u64 * MAP_GZIP_MAX_RATIO {
            return None;
        }
        if looks_like_html(&decoded) {
            return None;
        }
        String::from_utf8(decoded).ok()
    } else {
        let body = resp
            .into_body()
            .with_config()
            .limit(MAP_SITEMAP_MAX_BYTES)
            .read_to_string()
            .ok()?;
        if looks_like_html(body.as_bytes()) {
            return None;
        }
        Some(body)
    }
}

fn looks_like_html(bytes: &[u8]) -> bool {
    const DOCTYPE: &[u8] = b"<!doctype";
    const HTML: &[u8] = b"<html";
    const BOM: &[u8] = b"\xef\xbb\xbf";
    let mut prefix = bytes.get(..HTML_SNIFF_LEN).unwrap_or(bytes);
    if prefix.starts_with(BOM) {
        prefix = &prefix[BOM.len()..];
    }
    let prefix = prefix
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map_or(&[][..], |i| &prefix[i..]);
    prefix
        .get(..DOCTYPE.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(DOCTYPE))
        || prefix.get(..HTML.len()).is_some_and(|p| p.eq_ignore_ascii_case(HTML))
}

fn fetch_html(agent: &ureq::Agent, url: &Url) -> Option<String> {
    let resp = fetch_following_redirects(agent, url, url)?;
    resp.into_body()
        .with_config()
        .limit(MAP_HTML_MAX_BYTES)
        .read_to_string()
        .ok()
}

fn extract_links(html: &str, base: &Url) -> Vec<Url> {
    dom_query::Document::from(html)
        .select("a[href]")
        .iter()
        .filter_map(|el| {
            let href = el.attr("href")?;
            let href = href.trim();
            if href.is_empty() {
                return None;
            }
            let resolved = base.join(href).ok()?;
            matches!(resolved.scheme(), "http" | "https").then_some(resolved)
        })
        .collect()
}

enum SitemapEntry {
    Url { loc: String, lastmod: Option<String> },
    Sitemap { loc: String },
}

fn parse_sitemap(body: &str) -> Vec<SitemapEntry> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(body);
    let mut entries = Vec::new();
    let mut buf = Vec::new();
    let mut capture = Capture::Idle;
    let mut loc = String::new();
    let mut lastmod = String::new();
    let mut in_url = false;
    let mut in_sitemap = false;
    let mut depth: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"url" => {
                        in_url = true;
                        depth = 0;
                    }
                    b"sitemap" => {
                        in_sitemap = true;
                        depth = 0;
                    }
                    b"loc" if (in_url || in_sitemap) && depth == 0 => capture = Capture::Loc,
                    b"lastmod" if in_url && depth == 0 => capture = Capture::Lastmod,
                    _ if in_url || in_sitemap => depth += 1,
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if let Ok(text) = e.xml10_content() {
                    match capture {
                        Capture::Loc => loc.push_str(text.trim()),
                        Capture::Lastmod => lastmod.push_str(text.trim()),
                        Capture::Idle => {}
                    }
                } else {
                    loc.clear();
                    lastmod.clear();
                    capture = Capture::Idle;
                }
            }
            Ok(Event::GeneralRef(e)) => {
                let resolved = match &*e {
                    b"amp" => "&",
                    b"lt" => "<",
                    b"gt" => ">",
                    b"quot" => "\"",
                    b"apos" => "'",
                    _ => "",
                };
                match capture {
                    Capture::Loc => loc.push_str(resolved),
                    Capture::Lastmod => lastmod.push_str(resolved),
                    Capture::Idle => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"url" if in_url => {
                        if !loc.is_empty() {
                            let lm = if lastmod.is_empty() {
                                None
                            } else {
                                Some(std::mem::take(&mut lastmod))
                            };
                            entries.push(SitemapEntry::Url {
                                loc: std::mem::take(&mut loc),
                                lastmod: lm,
                            });
                        }
                        loc.clear();
                        lastmod.clear();
                        in_url = false;
                    }
                    b"sitemap" if in_sitemap => {
                        if !loc.is_empty() {
                            entries.push(SitemapEntry::Sitemap {
                                loc: std::mem::take(&mut loc),
                            });
                        }
                        loc.clear();
                        lastmod.clear();
                        in_sitemap = false;
                    }
                    b"loc" | b"lastmod" if capture != Capture::Idle => capture = Capture::Idle,
                    _ if depth > 0 => depth -= 1,
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    entries
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Capture {
    Idle,
    Loc,
    Lastmod,
}

fn validate_entry(
    loc: &str,
    lastmod: Option<String>,
    seed: &Url,
    robots: &RobotsPolicy,
    opts: &MapConfig,
    visited: &mut HashSet<String>,
) -> Option<MapEntry> {
    if loc.len() > MAP_URL_MAX_LEN {
        return None;
    }
    let url = net::validate_url(loc).ok()?;
    if !is_same_site(seed, &url) {
        return None;
    }
    if !robots.is_allowed(&url) {
        return None;
    }
    if !matches_scope(&url, opts.include.as_ref(), opts.exclude.as_ref()) {
        return None;
    }
    let normalized = normalize_url(&url);
    if !visited.insert(normalized.clone()) {
        return None;
    }
    Some(MapEntry {
        url: normalized,
        lastmod,
    })
}

async fn throttle(last_fetch: &mut Instant) {
    let elapsed = last_fetch.elapsed();
    if elapsed < MAP_MIN_FETCH_INTERVAL {
        tokio::time::sleep(MAP_MIN_FETCH_INTERVAL.saturating_sub(elapsed)).await;
    }
    *last_fetch = Instant::now();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(seed: &str) -> MapConfig {
        MapConfig {
            seed: Url::parse(seed).unwrap(),
            limit: 100,
            include: None,
            exclude: None,
            user_agent: None,
            timeout: Duration::from_secs(30),
            no_fallback: false,
        }
    }

    #[test]
    fn parse_urlset() {
        let xml = r#"<?xml version="1.0"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/a</loc><lastmod>2026-01-01</lastmod></url>
  <url><loc>https://example.com/b</loc></url>
</urlset>"#;
        let entries = parse_sitemap(xml);
        assert_eq!(entries.len(), 2);
        match &entries[0] {
            SitemapEntry::Url { loc, lastmod } => {
                assert_eq!(loc, "https://example.com/a");
                assert_eq!(lastmod.as_deref(), Some("2026-01-01"));
            }
            SitemapEntry::Sitemap { .. } => panic!("expected Url"),
        }
        match &entries[1] {
            SitemapEntry::Url { loc, lastmod } => {
                assert_eq!(loc, "https://example.com/b");
                assert!(lastmod.is_none());
            }
            SitemapEntry::Sitemap { .. } => panic!("expected Url"),
        }
    }

    #[test]
    fn parse_sitemapindex() {
        let xml = r#"<?xml version="1.0"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap><loc>https://example.com/sitemap1.xml</loc></sitemap>
  <sitemap><loc>https://example.com/sitemap2.xml</loc></sitemap>
</sitemapindex>"#;
        let entries = parse_sitemap(xml);
        assert_eq!(entries.len(), 2);
        match &entries[0] {
            SitemapEntry::Sitemap { loc } => assert_eq!(loc, "https://example.com/sitemap1.xml"),
            SitemapEntry::Url { .. } => panic!("expected Sitemap"),
        }
    }

    #[test]
    fn parse_handles_xml_entities() {
        let xml = r"<urlset><url><loc>https://example.com/a?b=1&amp;c=2</loc></url></urlset>";
        let entries = parse_sitemap(xml);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            SitemapEntry::Url { loc, .. } => assert_eq!(loc, "https://example.com/a?b=1&c=2"),
            SitemapEntry::Sitemap { .. } => panic!("expected Url"),
        }
    }

    #[test]
    fn parse_handles_namespaced_tags() {
        let xml = r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"
                xmlns:image="http://www.google.com/schemas/sitemap-image/1.1">
  <url>
    <loc>https://example.com/page</loc>
    <image:image><image:loc>https://example.com/img.png</image:loc></image:image>
  </url>
</urlset>"#;
        let entries = parse_sitemap(xml);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            SitemapEntry::Url { loc, .. } => assert_eq!(loc, "https://example.com/page"),
            SitemapEntry::Sitemap { .. } => panic!("expected Url"),
        }
    }

    #[test]
    fn parse_loc_after_nested_extension() {
        let xml = r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"
                xmlns:image="http://www.google.com/schemas/sitemap-image/1.1">
  <url>
    <image:image><image:loc>https://example.com/img.png</image:loc></image:image>
    <loc>https://example.com/page</loc>
    <lastmod>2026-01-01</lastmod>
  </url>
</urlset>"#;
        let entries = parse_sitemap(xml);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            SitemapEntry::Url { loc, lastmod } => {
                assert_eq!(loc, "https://example.com/page");
                assert_eq!(lastmod.as_deref(), Some("2026-01-01"));
            }
            SitemapEntry::Sitemap { .. } => panic!("expected Url"),
        }
    }

    #[test]
    fn parse_empty_body_returns_empty() {
        assert!(parse_sitemap("").is_empty());
        assert!(parse_sitemap("<html><body>Not Found</body></html>").is_empty());
    }

    #[test]
    fn looks_like_html_detects_variants() {
        assert!(looks_like_html(b"<!DOCTYPE html>"));
        assert!(looks_like_html(b"<!doctype html>"));
        assert!(looks_like_html(b"<html lang=\"en\">"));
        assert!(looks_like_html(b"<HTML>"));
        assert!(looks_like_html(b"\xef\xbb\xbf<!DOCTYPE html>"));
        assert!(looks_like_html(b"  \n<!doctype html>"));
        assert!(looks_like_html(b"\xef\xbb\xbf  <html>"));
        assert!(!looks_like_html(b"<?xml version=\"1.0\"?>"));
        assert!(!looks_like_html(b"<urlset>"));
        assert!(!looks_like_html(b""));
    }

    #[test]
    fn validate_entry_rejects_private_ip() {
        let opts = test_config("https://example.com");

        let mut visited = HashSet::new();
        let result = validate_entry(
            "http://127.0.0.1/secret",
            None,
            &opts.seed,
            &RobotsPolicy::Unavailable,
            &opts,
            &mut visited,
        );
        assert!(result.is_none());
    }

    #[test]
    fn validate_entry_rejects_cross_site() {
        let opts = test_config("https://example.com");
        let robots = RobotsPolicy::Unavailable;
        let mut visited = HashSet::new();
        let result = validate_entry("https://evil.com/page", None, &opts.seed, &robots, &opts, &mut visited);
        assert!(result.is_none());
    }

    #[test]
    fn validate_entry_deduplicates() {
        let opts = test_config("https://example.com");
        let robots = RobotsPolicy::Unavailable;
        let mut visited = HashSet::new();
        let first = validate_entry(
            "https://example.com/page",
            None,
            &opts.seed,
            &robots,
            &opts,
            &mut visited,
        );
        assert!(first.is_some());
        let second = validate_entry(
            "https://example.com/page",
            None,
            &opts.seed,
            &robots,
            &opts,
            &mut visited,
        );
        assert!(second.is_none());
    }

    #[test]
    fn validate_entry_rejects_long_url() {
        let opts = test_config("https://example.com");
        let robots = RobotsPolicy::Unavailable;
        let mut visited = HashSet::new();
        let long_url = format!("https://example.com/{}", "a".repeat(MAP_URL_MAX_LEN));
        let result = validate_entry(&long_url, None, &opts.seed, &robots, &opts, &mut visited);
        assert!(result.is_none());
    }

    #[test]
    fn discover_sitemaps_includes_robots_and_default() {
        let seed = Url::parse("https://example.com").unwrap();
        let robots = RobotsPolicy::Rules(RobotsRules {
            rules: Vec::new(),
            sitemaps: vec![Url::parse("https://example.com/custom-sitemap.xml").unwrap()],
        });
        let sitemaps = discover_sitemaps(&robots, &seed);
        assert_eq!(sitemaps.len(), 2);
        assert_eq!(sitemaps[0].as_str(), "https://example.com/custom-sitemap.xml");
        assert_eq!(sitemaps[1].as_str(), "https://example.com/sitemap.xml");
    }

    #[test]
    fn discover_sitemaps_deduplicates_default() {
        let seed = Url::parse("https://example.com").unwrap();
        let robots = RobotsPolicy::Rules(RobotsRules {
            rules: Vec::new(),
            sitemaps: vec![Url::parse("https://example.com/sitemap.xml").unwrap()],
        });
        let sitemaps = discover_sitemaps(&robots, &seed);
        assert_eq!(sitemaps.len(), 1);
    }

    mod integration {
        use super::super::*;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        #[tokio::test]
        async fn fetch_sitemap_parses_urlset() {
            let server = MockServer::start().await;
            let xml = r#"<?xml version="1.0"?><urlset><url><loc>https://example.com/a</loc></url></urlset>"#;
            Mock::given(method("GET"))
                .and(path("/sitemap.xml"))
                .respond_with(ResponseTemplate::new(200).set_body_string(xml))
                .mount(&server)
                .await;

            let agent = build_agent("test/1.0", Duration::from_secs(5));
            let url = Url::parse(&format!("{}/sitemap.xml", server.uri())).unwrap();
            let body = tokio::task::spawn_blocking(move || fetch_sitemap(&agent, &url, &url))
                .await
                .unwrap();

            let entries = parse_sitemap(&body.unwrap());
            assert_eq!(entries.len(), 1);
        }

        #[tokio::test]
        async fn fetch_sitemap_rejects_html_error_page() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/sitemap.xml"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_string("<!DOCTYPE html><html><body>Not Found</body></html>"),
                )
                .mount(&server)
                .await;

            let agent = build_agent("test/1.0", Duration::from_secs(5));
            let url = Url::parse(&format!("{}/sitemap.xml", server.uri())).unwrap();
            let body = tokio::task::spawn_blocking(move || fetch_sitemap(&agent, &url, &url))
                .await
                .unwrap();

            assert!(body.is_none());
        }

        #[tokio::test]
        async fn fetch_sitemap_returns_none_on_404() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/sitemap.xml"))
                .respond_with(ResponseTemplate::new(404))
                .mount(&server)
                .await;

            let agent = build_agent("test/1.0", Duration::from_secs(5));
            let url = Url::parse(&format!("{}/sitemap.xml", server.uri())).unwrap();
            let body = tokio::task::spawn_blocking(move || fetch_sitemap(&agent, &url, &url))
                .await
                .unwrap();

            assert!(body.is_none());
        }

        #[tokio::test]
        async fn fetch_sitemap_handles_gzip() {
            use flate2::Compression;
            use flate2::write::GzEncoder;
            use std::io::Write;

            let server = MockServer::start().await;
            let xml = r#"<?xml version="1.0"?><urlset><url><loc>https://example.com/gz</loc></url></urlset>"#;
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(xml.as_bytes()).unwrap();
            let compressed = encoder.finish().unwrap();

            Mock::given(method("GET"))
                .and(path("/sitemap.xml.gz"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_bytes(compressed)
                        .insert_header("content-type", "application/gzip"),
                )
                .mount(&server)
                .await;

            let agent = build_agent("test/1.0", Duration::from_secs(5));
            let url = Url::parse(&format!("{}/sitemap.xml.gz", server.uri())).unwrap();
            let body = tokio::task::spawn_blocking(move || fetch_sitemap(&agent, &url, &url))
                .await
                .unwrap();

            let entries = parse_sitemap(&body.unwrap());
            assert_eq!(entries.len(), 1);
        }

        #[tokio::test]
        async fn fetch_html_extracts_links() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_string(r#"<html><body><a href="/link">x</a></body></html>"#),
                )
                .mount(&server)
                .await;

            let agent = build_agent("test/1.0", Duration::from_secs(5));
            let seed = Url::parse(&server.uri()).unwrap();
            let html = tokio::task::spawn_blocking({
                let seed = seed.clone();
                move || fetch_html(&agent, &seed)
            })
            .await
            .unwrap()
            .unwrap();

            let links = extract_links(&html, &seed);
            assert_eq!(links.len(), 1);
        }
    }
}
