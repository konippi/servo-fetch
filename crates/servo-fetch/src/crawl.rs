//! Site crawling — BFS link traversal with scope, robots.txt, and rate limiting.

use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use url::Url;

use crate::bridge;
use crate::net;
use crate::robots::{RobotsPolicy, RobotsRules};
use crate::scope::{is_same_site, matches_scope, normalize_url};

const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;
const MIN_CRAWL_INTERVAL: Duration = Duration::from_millis(500);

/// Crawl configuration.
pub(crate) struct CrawlOptions {
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

#[expect(clippy::too_many_lines, reason = "BFS loop with inline error handling")]
pub(crate) async fn run(opts: CrawlOptions, mut on_page: impl FnMut(&CrawlPageResult)) -> Vec<CrawlPageResult> {
    let robots = tokio::task::spawn_blocking({
        let seed = opts.seed.clone();
        let user_agent = opts.user_agent.clone();
        move || RobotsRules::fetch(&seed, user_agent.as_deref())
    })
    .await
    .unwrap_or(RobotsPolicy::Unreachable);

    let mut frontier = Frontier::new(&opts.seed);
    let mut results = Vec::new();
    let mut last_fetch = Instant::now()
        .checked_sub(MIN_CRAWL_INTERVAL)
        .unwrap_or_else(Instant::now);

    while let Some((url, depth)) = frontier.pop() {
        if results.len() >= opts.limit {
            break;
        }

        let elapsed = last_fetch.elapsed();
        if elapsed < MIN_CRAWL_INTERVAL {
            tokio::time::sleep(MIN_CRAWL_INTERVAL.saturating_sub(elapsed)).await;
        }
        last_fetch = Instant::now();

        let url_str = url.to_string();
        let timeout = opts.timeout_secs;
        let settle = opts.settle_ms;
        let user_agent = opts.user_agent.clone();

        let page = match tokio::task::spawn_blocking(move || {
            bridge::fetch_page(bridge::FetchOptions {
                url: &url_str,
                timeout_secs: timeout,
                settle_ms: settle,
                mode: bridge::FetchMode::Content { include_a11y: false },
                user_agent: user_agent.as_deref(),
            })
        })
        .await
        {
            Ok(Ok(p)) => p,
            result => {
                let msg = match result {
                    Ok(Err(e)) => format!("{e:#}"),
                    Err(e) => format!("{e}"),
                    Ok(Ok(_)) => unreachable!(),
                };
                let r = error_result(&url, depth, msg);
                on_page(&r);
                results.push(r);
                continue;
            }
        };

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
            continue;
        }

        let links = extract_links_from_html(html, &url);
        let links_found = links.len();

        if depth < opts.max_depth {
            for link in &links {
                if results.len() + frontier.pending() >= opts.limit {
                    break;
                }
                if !is_same_site(&opts.seed, link)
                    || net::validate_url(link.as_str()).is_err()
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

        let r = CrawlPageResult {
            url: url.to_string(),
            depth,
            status: CrawlStatus::Ok,
            title,
            content: content.map(|c| crate::sanitize::sanitize(&c).into_owned()),
            error: None,
            links_found,
        };
        on_page(&r);
        results.push(r);
    }

    results
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
