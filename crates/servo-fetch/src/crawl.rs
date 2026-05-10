//! Site crawling — BFS link traversal with scope, robots.txt, and rate limiting.

use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::time::Duration;

use tokio::task::JoinSet;
use tokio::time::{MissedTickBehavior, interval};
use url::Url;

use crate::bridge::{self, PageFetcher};
use crate::net;
use crate::robots::RobotsPolicy;
use crate::scope::{is_same_site, matches_scope, normalize_url};

const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;

/// Options for crawling a site.
#[must_use = "options do nothing until passed to crawl() or crawl_each()"]
#[derive(Debug, Clone)]
pub struct CrawlOptions {
    pub(crate) url: String,
    pub(crate) limit: usize,
    pub(crate) max_depth: usize,
    pub(crate) timeout: Duration,
    pub(crate) settle: Duration,
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
    pub(crate) selector: Option<String>,
    pub(crate) json: bool,
    pub(crate) user_agent: Option<String>,
    pub(crate) concurrency: usize,
    pub(crate) delay: Option<Duration>,
}

impl CrawlOptions {
    /// Create crawl options for the given seed URL.
    pub fn new(url: &str) -> Self {
        Self {
            url: url.into(),
            limit: 50,
            max_depth: 3,
            timeout: Duration::from_secs(30),
            settle: Duration::ZERO,
            include: Vec::new(),
            exclude: Vec::new(),
            selector: None,
            json: false,
            user_agent: None,
            concurrency: 1,
            delay: Some(Duration::from_millis(500)),
        }
    }

    /// Maximum number of pages to crawl (default: 50).
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }

    /// Maximum link depth from the seed URL (default: 3).
    pub fn max_depth(mut self, n: usize) -> Self {
        self.max_depth = n;
        self
    }

    /// Page load timeout per page (default: 30s).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Extra wait after load event per page (default: 0).
    pub fn settle(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }

    /// URL path glob patterns to include (e.g. `"/docs/**"`).
    pub fn include(mut self, patterns: &[&str]) -> Self {
        self.include = patterns.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// URL path glob patterns to exclude (e.g. `"/docs/archive/**"`).
    pub fn exclude(mut self, patterns: &[&str]) -> Self {
        self.exclude = patterns.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// Output crawled content as JSON instead of Markdown.
    pub fn json(mut self, json: bool) -> Self {
        self.json = json;
        self
    }

    /// CSS selector to extract a specific section per page.
    pub fn selector(mut self, selector: impl Into<String>) -> Self {
        self.selector = Some(selector.into());
        self
    }

    /// Override the User-Agent string for all pages in this crawl.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(net::sanitize_user_agent(ua.into()));
        self
    }

    /// Maximum parallel fetches (default: 1). Values below 1 are clamped to 1.
    /// Results are yielded in completion order when greater than 1.
    pub fn concurrency(mut self, n: usize) -> Self {
        self.concurrency = n.max(1);
        self
    }

    /// Minimum dispatch interval (default: `Some(500ms)`). `None` disables rate limiting.
    pub fn delay(mut self, delay: Option<Duration>) -> Self {
        self.delay = delay;
        self
    }
}

/// Result for a single crawled page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CrawlResult {
    /// URL of the crawled page.
    pub url: String,
    /// Link depth from the seed URL.
    pub depth: usize,
    /// Page content if successful, or error if failed.
    pub outcome: Result<CrawlPage, CrawlError>,
}

/// Successfully crawled page.
#[derive(Debug, Clone)]
pub struct CrawlPage {
    /// Page title.
    pub title: Option<String>,
    /// Extracted content (Markdown or JSON depending on options).
    pub content: String,
    /// Number of links discovered on this page.
    pub links_found: usize,
}

/// Error from a failed crawl attempt.
#[derive(Debug, Clone)]
pub struct CrawlError {
    /// Error message.
    pub message: String,
}

impl std::fmt::Display for CrawlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CrawlError {}

impl serde::Serialize for CrawlResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match &self.outcome {
            Ok(page) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("url", &self.url)?;
                map.serialize_entry("depth", &self.depth)?;
                map.serialize_entry("status", "ok")?;
                if let Some(t) = &page.title {
                    map.serialize_entry("title", t)?;
                }
                map.serialize_entry("content", &page.content)?;
                map.serialize_entry("links_found", &page.links_found)?;
                map.end()
            }
            Err(e) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("url", &self.url)?;
                map.serialize_entry("depth", &self.depth)?;
                map.serialize_entry("status", "error")?;
                map.serialize_entry("error", &e.message)?;
                map.end()
            }
        }
    }
}

impl CrawlResult {
    fn from_internal(r: &CrawlPageResult) -> Self {
        let outcome = match r.status {
            CrawlStatus::Ok => Ok(CrawlPage {
                title: r.title.clone(),
                content: r.content.clone().unwrap_or_default(),
                links_found: r.links_found,
            }),
            CrawlStatus::Error => Err(CrawlError {
                message: r.error.clone().unwrap_or_default(),
            }),
        };
        Self {
            url: r.url.clone(),
            depth: r.depth,
            outcome,
        }
    }
}

/// Crawl a site, invoking `on_page` for each result as it arrives.
#[allow(clippy::needless_pass_by_value)]
pub fn crawl_each(opts: CrawlOptions, mut on_page: impl FnMut(&CrawlResult)) -> crate::error::Result<()> {
    net::ensure_crypto_provider();
    let plan = build_crawl_plan(&opts)?;
    crate::runtime::block_on(async {
        let robots = tokio::task::spawn_blocking({
            let seed = plan.seed.clone();
            let user_agent = plan.user_agent.clone();
            let timeout = Duration::from_secs(plan.timeout_secs);
            move || crate::robots::RobotsRules::fetch(&seed, user_agent.as_deref(), timeout)
        })
        .await
        .unwrap_or(RobotsPolicy::Unreachable);
        run(plan, robots, &bridge::ServoFetcher, |r| {
            on_page(&CrawlResult::from_internal(r));
        })
        .await
    })
    .map_err(|e| crate::error::Error::Engine(e.to_string()))?;
    Ok(())
}

/// Crawl a site and collect all results.
#[allow(clippy::needless_pass_by_value)]
pub fn crawl(opts: CrawlOptions) -> crate::error::Result<Vec<CrawlResult>> {
    let mut results = Vec::new();
    crawl_each(opts, |r| results.push(r.clone()))?;
    Ok(results)
}

fn build_crawl_plan(opts: &CrawlOptions) -> crate::error::Result<CrawlPlan> {
    let seed = net::validate_url(&opts.url)?;
    let include = if opts.include.is_empty() {
        None
    } else {
        Some(crate::scope::build_globset(&opts.include)?)
    };
    let exclude = if opts.exclude.is_empty() {
        None
    } else {
        Some(crate::scope::build_globset(&opts.exclude)?)
    };
    Ok(CrawlPlan {
        seed,
        limit: opts.limit,
        max_depth: opts.max_depth,
        timeout_secs: opts.timeout.as_secs().max(1),
        settle_ms: u64::try_from(opts.settle.as_millis()).unwrap_or(u64::MAX),
        include,
        exclude,
        selector: opts.selector.clone(),
        json: opts.json,
        user_agent: opts.user_agent.clone(),
        concurrency: opts.concurrency,
        delay: opts.delay,
    })
}

/// Crawl configuration.
pub(crate) struct CrawlPlan {
    pub seed: Url,
    pub limit: usize,
    pub max_depth: usize,
    pub timeout_secs: u64,
    pub settle_ms: u64,
    pub include: Option<globset::GlobSet>,
    pub exclude: Option<globset::GlobSet>,
    pub selector: Option<String>,
    pub json: bool,
    pub user_agent: Option<String>,
    /// Parallel fetch limit (clamped to >=1; yields in completion order when >1).
    pub concurrency: usize,
    /// Dispatch interval; `None` disables rate limiting.
    pub delay: Option<Duration>,
}

/// Result for a single crawled page.
#[derive(serde::Serialize)]
pub(crate) struct CrawlPageResult {
    pub url: String,
    pub depth: usize,
    pub status: CrawlStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub links_found: usize,
}

/// Status of a crawled page.
#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CrawlStatus {
    Ok,
    Error,
}

struct Frontier {
    queue: VecDeque<(Url, usize)>,
    visited: HashSet<String>,
    content_hashes: HashSet<u64>,
}

impl Frontier {
    fn new(seed: &Url) -> Self {
        Self {
            queue: VecDeque::from([(seed.clone(), 0)]),
            visited: HashSet::from([normalize_url(seed)]),
            content_hashes: HashSet::new(),
        }
    }

    fn try_enqueue(&mut self, url: Url, depth: usize) -> bool {
        if self.visited.insert(normalize_url(&url)) {
            self.queue.push_back((url, depth));
            true
        } else {
            false
        }
    }

    fn pop(&mut self) -> Option<(Url, usize)> {
        self.queue.pop_front()
    }

    fn is_duplicate_content(&mut self, content: &str) -> bool {
        let mut h = std::hash::DefaultHasher::new();
        content.hash(&mut h);
        !self.content_hashes.insert(h.finish())
    }

    fn pending(&self) -> usize {
        self.queue.len()
    }
}

fn extract_links_from_html(html: &str, base: &Url) -> Vec<Url> {
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

pub(crate) async fn run(
    opts: CrawlPlan,
    robots: RobotsPolicy,
    fetcher: &(impl PageFetcher + Clone),
    mut on_page: impl FnMut(&CrawlPageResult),
) -> Vec<CrawlPageResult> {
    let mut frontier = Frontier::new(&opts.seed);
    let mut results = Vec::new();
    let mut in_flight: JoinSet<FetchOutcome> = JoinSet::new();

    // `Delay` keeps the steady-state rate correct after fetches exceed `delay`.
    let mut ticker = opts.delay.map(|period| {
        let mut t = interval(period);
        t.set_missed_tick_behavior(MissedTickBehavior::Delay);
        t
    });

    let concurrency = opts.concurrency.max(1);

    loop {
        while in_flight.len() < concurrency && results.len() + in_flight.len() < opts.limit {
            let Some((url, depth)) = frontier.pop() else {
                break;
            };
            if let Some(t) = ticker.as_mut() {
                t.tick().await;
            }
            spawn_fetch(&mut in_flight, fetcher, &opts, url, depth);
        }

        let outcome = match in_flight.join_next().await {
            None => break,
            Some(Ok(o)) => o,
            Some(Err(e)) if e.is_panic() => {
                tracing::error!(err = %e, "crawl fetch task panicked");
                continue;
            }
            Some(Err(e)) => {
                tracing::warn!(err = %e, "crawl fetch task cancelled");
                continue;
            }
        };

        let FetchOutcome { url, depth, result } = outcome;
        let page = match result {
            Ok(p) => p,
            Err(msg) => {
                let r = error_result(&url, depth, msg);
                on_page(&r);
                results.push(r);
                continue;
            }
        };

        // +1 counts the current page, which is about to be pushed.
        let budget_used = results.len() + in_flight.len() + 1;
        if let Some(r) = process_ok_fetch(&url, depth, &page, &mut frontier, &robots, &opts, budget_used) {
            on_page(&r);
            results.push(r);
        }
    }

    results
}

fn spawn_fetch(
    in_flight: &mut JoinSet<FetchOutcome>,
    fetcher: &(impl PageFetcher + Clone),
    opts: &CrawlPlan,
    url: Url,
    depth: usize,
) {
    let url_str = url.to_string();
    let timeout = opts.timeout_secs;
    let settle = opts.settle_ms;
    let user_agent = opts.user_agent.clone();
    let f = fetcher.clone();
    in_flight.spawn_blocking(move || FetchOutcome {
        url,
        depth,
        result: f
            .fetch_page(bridge::FetchOptions {
                url: &url_str,
                timeout_secs: timeout,
                settle_ms: settle,
                mode: bridge::FetchMode::Content { include_a11y: false },
                user_agent: user_agent.as_deref(),
            })
            .map_err(|e| format!("{e:#}")),
    });
}

/// Build a `CrawlPageResult` and enqueue discovered links.
fn process_ok_fetch(
    url: &Url,
    depth: usize,
    page: &bridge::ServoPage,
    frontier: &mut Frontier,
    robots: &RobotsPolicy,
    opts: &CrawlPlan,
    budget_used: usize,
) -> Option<CrawlPageResult> {
    let html = if page.html.len() > MAX_HTML_BYTES {
        &page.html[..crate::sanitize::floor_char_boundary(&page.html, MAX_HTML_BYTES)]
    } else {
        &page.html
    };

    let input = crate::extract::ExtractInput::new(html, url.as_str())
        .with_layout_json(page.layout_json.as_deref())
        .with_inner_text(page.inner_text.as_deref())
        .with_selector(opts.selector.as_deref());

    let content = if opts.json {
        crate::extract::extract_json(&input).ok()
    } else {
        crate::extract::extract_text(&input).ok()
    };

    if content.as_ref().is_some_and(|c| frontier.is_duplicate_content(c)) {
        return None;
    }

    let links = extract_links_from_html(html, url);
    let links_found = links.len();

    if depth < opts.max_depth {
        for link in &links {
            if budget_used + frontier.pending() >= opts.limit {
                break;
            }
            if !is_same_site(&opts.seed, link)
                || net::validate_url_with_policy(link.as_str(), bridge::engine_policy()).is_err()
                || !robots.is_allowed(link)
                || !matches_scope(link, opts.include.as_ref(), opts.exclude.as_ref())
            {
                continue;
            }
            frontier.try_enqueue(link.clone(), depth + 1);
        }
    }

    let title = {
        let doc = dom_query::Document::from(html);
        let t = doc.select("title").text().to_string();
        (!t.is_empty()).then_some(t)
    };

    Some(CrawlPageResult {
        url: url.to_string(),
        depth,
        status: CrawlStatus::Ok,
        title,
        content: content.map(|c| crate::sanitize::sanitize(&c).into_owned()),
        error: None,
        links_found,
    })
}

/// Fetch result crossing the `JoinSet` boundary; error is pre-stringified for `Send`.
struct FetchOutcome {
    url: Url,
    depth: usize,
    result: Result<bridge::ServoPage, String>,
}

fn error_result(url: &Url, depth: usize, error: String) -> CrawlPageResult {
    CrawlPageResult {
        url: url.to_string(),
        depth,
        status: CrawlStatus::Error,
        title: None,
        content: None,
        error: Some(error),
        links_found: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn crawl_options_defaults() {
        let opts = CrawlOptions::new("https://example.com");
        assert_eq!(opts.url, "https://example.com");
        assert_eq!(opts.limit, 50);
        assert_eq!(opts.max_depth, 3);
        assert_eq!(opts.timeout, Duration::from_secs(30));
        assert!(opts.include.is_empty());
        assert!(opts.exclude.is_empty());
        assert_eq!(opts.concurrency, 1);
        assert_eq!(opts.delay, Some(Duration::from_millis(500)));
    }

    #[test]
    fn crawl_options_chaining() {
        let opts = CrawlOptions::new("https://example.com")
            .limit(100)
            .max_depth(5)
            .timeout(Duration::from_secs(60))
            .include(&["/docs/**"])
            .exclude(&["/docs/archive/**"])
            .concurrency(4)
            .delay(None);
        assert_eq!(opts.limit, 100);
        assert_eq!(opts.max_depth, 5);
        assert_eq!(opts.include, vec!["/docs/**"]);
        assert_eq!(opts.exclude, vec!["/docs/archive/**"]);
        assert_eq!(opts.concurrency, 4);
        assert_eq!(opts.delay, None);
    }

    #[test]
    fn crawl_options_concurrency_clamps_below_one() {
        let opts = CrawlOptions::new("https://example.com").concurrency(0);
        assert_eq!(opts.concurrency, 1);
    }

    #[test]
    fn crawl_options_delay_custom_value() {
        let opts = CrawlOptions::new("https://example.com").delay(Some(Duration::from_secs(2)));
        assert_eq!(opts.delay, Some(Duration::from_secs(2)));
    }

    #[test]
    fn crawl_user_agent_sanitizes_crlf() {
        let opts = CrawlOptions::new("https://example.com").user_agent("Crawler\r\n/2.0");
        assert_eq!(opts.user_agent.as_deref(), Some("Crawler  /2.0"));
    }

    #[derive(Clone)]
    struct MockFetcher(Arc<HashMap<String, String>>);

    impl MockFetcher {
        fn new(pages: &[(&str, &str)]) -> Self {
            Self(Arc::new(
                pages.iter().map(|(u, h)| (u.to_string(), h.to_string())).collect(),
            ))
        }
    }

    impl PageFetcher for MockFetcher {
        fn fetch_page(&self, opts: bridge::FetchOptions<'_>) -> anyhow::Result<bridge::ServoPage> {
            self.0
                .get(opts.url)
                .map(|html| bridge::ServoPage {
                    html: html.clone(),
                    ..Default::default()
                })
                .ok_or_else(|| anyhow::anyhow!("not found: {}", opts.url))
        }
    }

    fn page(links: &[&str]) -> String {
        use std::fmt::Write;
        let mut anchors = String::new();
        for l in links {
            write!(anchors, r#"<a href="{l}">link</a>"#).unwrap();
        }
        format!("<html><head><title>Test</title></head><body>{anchors}</body></html>")
    }

    /// Leaf page with unique body to avoid content-hash dedup.
    fn distinct_page(tag: &str) -> String {
        format!("<html><head><title>{tag}</title></head><body>page {tag}</body></html>")
    }

    /// Test helper: build `CrawlPlan`, run, assert. `delay=None` keeps tests fast.
    async fn check(
        pages: &[(&str, &str)],
        configure: impl FnOnce(&mut CrawlPlan),
        assert: impl FnOnce(&[CrawlPageResult]),
    ) {
        let fetcher = MockFetcher::new(pages);
        let seed = pages[0].0;
        let mut opts = CrawlPlan {
            seed: Url::parse(seed).unwrap(),
            limit: 50,
            max_depth: 3,
            timeout_secs: 30,
            settle_ms: 0,
            include: None,
            exclude: None,
            selector: None,
            json: false,
            user_agent: None,
            concurrency: 1,
            delay: None,
        };
        configure(&mut opts);
        let results = run(opts, RobotsPolicy::Unavailable, &fetcher, |_| {}).await;
        assert(&results);
    }

    #[tokio::test]
    async fn crawl_single_page() {
        check(
            &[("https://example.com/", &page(&[]))],
            |_| {},
            |r| {
                assert_eq!(r.len(), 1);
                assert_eq!(r[0].url, "https://example.com/");
            },
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_follows_links() {
        check(
            &[
                ("https://example.com/", &page(&["/a", "/b"])),
                (
                    "https://example.com/a",
                    "<html><head><title>A</title></head><body>page a</body></html>",
                ),
                (
                    "https://example.com/b",
                    "<html><head><title>B</title></head><body>page b</body></html>",
                ),
            ],
            |_| {},
            |r| assert_eq!(r.len(), 3),
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_respects_depth_limit() {
        check(
            &[
                ("https://example.com/", &page(&["/a"])),
                ("https://example.com/a", &page(&["/b"])),
                ("https://example.com/b", &page(&["/c"])),
                ("https://example.com/c", &page(&[])),
            ],
            |o| o.max_depth = 1,
            |r| assert_eq!(r.len(), 2),
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_respects_limit() {
        check(
            &[
                ("https://example.com/", &page(&["/a", "/b", "/c"])),
                ("https://example.com/a", &page(&[])),
                ("https://example.com/b", &page(&[])),
                ("https://example.com/c", &page(&[])),
            ],
            |o| o.limit = 2,
            |r| assert_eq!(r.len(), 2),
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_skips_cross_site_links() {
        check(
            &[
                ("https://example.com/", &page(&["https://other.com/x"])),
                ("https://other.com/x", &page(&[])),
            ],
            |_| {},
            |r| assert_eq!(r.len(), 1),
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_deduplicates_urls() {
        check(
            &[
                ("https://example.com/", &page(&["/a", "/a", "/a"])),
                ("https://example.com/a", &page(&["/"])),
            ],
            |_| {},
            |r| assert_eq!(r.len(), 2),
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_handles_fetch_errors() {
        check(
            &[("https://example.com/", &page(&["/missing"]))],
            |_| {},
            |r| {
                assert_eq!(r.len(), 2);
                assert!(matches!(r[1].status, CrawlStatus::Error));
                assert!(r[1].error.is_some());
            },
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_applies_include_glob() {
        check(
            &[
                ("https://example.com/", &page(&["/docs/a", "/blog/b"])),
                ("https://example.com/docs/a", &page(&[])),
                ("https://example.com/blog/b", &page(&[])),
            ],
            |o| o.include = Some(crate::scope::build_globset(&["/docs/**".into()]).unwrap()),
            |r| {
                assert_eq!(r.len(), 2);
                assert!(r.iter().any(|p| p.url == "https://example.com/docs/a"));
                assert!(!r.iter().any(|p| p.url == "https://example.com/blog/b"));
            },
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_applies_exclude_glob() {
        check(
            &[
                ("https://example.com/", &page(&["/public", "/secret/data"])),
                ("https://example.com/public", &page(&[])),
                ("https://example.com/secret/data", &page(&[])),
            ],
            |o| o.exclude = Some(crate::scope::build_globset(&["/secret/**".into()]).unwrap()),
            |r| {
                assert_eq!(r.len(), 2);
                assert!(!r.iter().any(|p| p.url == "https://example.com/secret/data"));
            },
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_deduplicates_content() {
        let same = "<html><head><title>Same</title></head><body>identical</body></html>";
        check(
            &[
                ("https://example.com/", &page(&["/a", "/b"])),
                ("https://example.com/a", same),
                ("https://example.com/b", same),
            ],
            |_| {},
            |r| assert_eq!(r.len(), 2),
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_concurrency_visits_all_pages() {
        check(
            &[
                ("https://example.com/", &page(&["/a", "/b", "/c", "/d"])),
                ("https://example.com/a", &distinct_page("a")),
                ("https://example.com/b", &distinct_page("b")),
                ("https://example.com/c", &distinct_page("c")),
                ("https://example.com/d", &distinct_page("d")),
            ],
            |o| o.concurrency = 4,
            |r| {
                assert_eq!(r.len(), 5);
                let urls: HashSet<&str> = r.iter().map(|p| p.url.as_str()).collect();
                for u in [
                    "https://example.com/",
                    "https://example.com/a",
                    "https://example.com/b",
                    "https://example.com/c",
                    "https://example.com/d",
                ] {
                    assert!(urls.contains(u), "missing {u}");
                }
            },
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_concurrency_respects_limit() {
        check(
            &[
                ("https://example.com/", &page(&["/a", "/b", "/c", "/d"])),
                ("https://example.com/a", &distinct_page("a")),
                ("https://example.com/b", &distinct_page("b")),
                ("https://example.com/c", &distinct_page("c")),
                ("https://example.com/d", &distinct_page("d")),
            ],
            |o| {
                o.concurrency = 4;
                o.limit = 3;
            },
            |r| assert_eq!(r.len(), 3),
        )
        .await;
    }

    #[tokio::test]
    async fn crawl_concurrency_one_preserves_bfs_order() {
        check(
            &[
                ("https://example.com/", &page(&["/a", "/b"])),
                ("https://example.com/a", &distinct_page("a")),
                ("https://example.com/b", &distinct_page("b")),
            ],
            |o| o.concurrency = 1,
            |r| {
                assert_eq!(r.len(), 3);
                assert_eq!(r[0].url, "https://example.com/");
                assert_eq!(r[1].url, "https://example.com/a");
                assert_eq!(r[2].url, "https://example.com/b");
            },
        )
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn crawl_delay_enforces_minimum_interval() {
        // 3 pages at 500ms delay = 2 ticks >= 1s (first dispatch is free).
        let start = tokio::time::Instant::now();
        check(
            &[
                ("https://example.com/", &page(&["/a", "/b"])),
                ("https://example.com/a", &distinct_page("a")),
                ("https://example.com/b", &distinct_page("b")),
            ],
            |o| {
                o.concurrency = 1;
                o.delay = Some(Duration::from_millis(500));
            },
            |r| assert_eq!(r.len(), 3),
        )
        .await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_secs(1),
            "expected >= 1s for 3 pages with 500ms delay, got {elapsed:?}"
        );
    }

    #[test]
    fn frontier_dedup() {
        let seed = Url::parse("https://example.com/").unwrap();
        let mut f = Frontier::new(&seed);
        assert!(!f.try_enqueue(seed, 0));
        let other = Url::parse("https://example.com/page").unwrap();
        assert!(f.try_enqueue(other.clone(), 1));
        assert!(!f.try_enqueue(other, 1));
    }

    #[test]
    fn frontier_pop_and_pending() {
        let seed = Url::parse("https://example.com/").unwrap();
        let mut f = Frontier::new(&seed);
        assert_eq!(f.pending(), 1);
        let (url, depth) = f.pop().unwrap();
        assert_eq!(url.as_str(), "https://example.com/");
        assert_eq!(depth, 0);
        assert_eq!(f.pending(), 0);
        assert!(f.pop().is_none());
    }

    #[test]
    fn extract_links_filters_dangerous_schemes() {
        let html = r#"<a href="https://example.com/a">A</a>
            <a href="javascript:void(0)">JS</a>
            <a href="JAVASCRIPT:alert(1)">JS upper</a>
            <a href="data:text/html,<h1>hi</h1>">Data</a>
            <a href="mailto:x@y.com">Mail</a>
            <a href="/relative">Rel</a>"#;
        let base = Url::parse("https://example.com/").unwrap();
        let links = extract_links_from_html(html, &base);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].as_str(), "https://example.com/a");
        assert_eq!(links[1].as_str(), "https://example.com/relative");
    }

    #[test]
    fn error_result_fields() {
        let url = Url::parse("https://example.com/fail").unwrap();
        let r = error_result(&url, 2, "timeout".into());
        assert!(matches!(r.status, CrawlStatus::Error));
        assert_eq!(r.error.as_deref(), Some("timeout"));
        assert!(r.content.is_none());
    }

    #[test]
    fn content_hash_dedup() {
        let seed = Url::parse("https://example.com/").unwrap();
        let mut f = Frontier::new(&seed);
        assert!(!f.is_duplicate_content("unique content"));
        assert!(f.is_duplicate_content("unique content"));
        assert!(!f.is_duplicate_content("different content"));
    }
}
