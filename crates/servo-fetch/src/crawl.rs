//! Site crawling — BFS link traversal with scope, robots.txt, and rate limiting.

use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use globset::{Glob, GlobSet, GlobSetBuilder};
use url::Url;

use crate::bridge;
use crate::net;

const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;
const ROBOTS_MAX_BYTES: u64 = 512 * 1024;
const ROBOTS_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_CRAWL_INTERVAL: Duration = Duration::from_millis(500);

/// Crawl configuration.
pub(crate) struct CrawlOptions {
    pub seed: Url,
    pub limit: usize,
    pub max_depth: usize,
    pub timeout_secs: u64,
    pub settle_ms: u64,
    pub include: Option<GlobSet>,
    pub exclude: Option<GlobSet>,
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

fn normalize_url(url: &Url) -> String {
    let mut u = url.clone();
    u.set_fragment(None);
    u.to_string()
}

fn is_same_site(seed: &Url, candidate: &Url) -> bool {
    match (registrable_domain(seed), registrable_domain(candidate)) {
        (Some(a), Some(b)) => a == b,
        _ => seed.host_str() == candidate.host_str(),
    }
}

fn registrable_domain(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    let domain = psl::domain(host.as_bytes())?;
    Some(std::str::from_utf8(domain.as_bytes()).ok()?.to_string())
}

fn matches_scope(url: &Url, include: Option<&GlobSet>, exclude: Option<&GlobSet>) -> bool {
    let path = url.path();
    if let Some(exc) = exclude {
        if exc.is_match(path) {
            return false;
        }
    }
    match include {
        Some(inc) => inc.is_match(path),
        None => true,
    }
}

/// Build a `GlobSet` from user-provided patterns.
pub(crate) fn build_globset(patterns: &[String]) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p)?);
    }
    Ok(builder.build()?)
}

fn strip_directive<'a>(line: &'a str, directive: &str) -> Option<&'a str> {
    let len = directive.len();
    if line.len() > len && line[..len].eq_ignore_ascii_case(directive) && line.as_bytes()[len] == b':' {
        Some(&line[len + 1..])
    } else {
        None
    }
}

fn robots_url(seed: &Url) -> Option<Url> {
    let mut base = seed.clone();
    base.set_username("").ok();
    base.set_password(None).ok();
    base.join("/robots.txt").ok()
}

fn product_token(user_agent: &str) -> &str {
    user_agent
        .split(|c: char| c == '/' || c.is_whitespace())
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("*")
}

/// Outcome of `RobotsRules::fetch`.
enum RobotsPolicy {
    Rules(RobotsRules),
    /// 4xx other than 401/403 — treat as no restrictions.
    Unavailable,
    /// Auth wall, server error, or network failure — fail closed.
    Unreachable,
}

impl RobotsPolicy {
    fn is_allowed(&self, url: &Url) -> bool {
        match self {
            Self::Rules(r) => r.is_allowed(url),
            Self::Unavailable => true,
            Self::Unreachable => false,
        }
    }
}

struct RobotsRules {
    rules: Vec<(bool, String)>,
}

impl RobotsRules {
    fn fetch(seed: &Url, user_agent: Option<&str>) -> RobotsPolicy {
        let Some(url) = robots_url(seed) else {
            return RobotsPolicy::Unreachable;
        };
        let ua = user_agent.unwrap_or_else(|| bridge::default_user_agent());
        let agent = ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .max_redirects(0)
                .timeout_global(Some(ROBOTS_TIMEOUT))
                .user_agent(ua)
                .build(),
        );
        match agent.get(url.as_str()).call() {
            Ok(resp) => resp
                .into_body()
                .with_config()
                .limit(ROBOTS_MAX_BYTES)
                .read_to_string()
                .map_or(RobotsPolicy::Unreachable, |body| {
                    RobotsPolicy::Rules(Self::parse(&body, product_token(ua)))
                }),
            Err(ureq::Error::StatusCode(401 | 403 | 429)) => RobotsPolicy::Unreachable,
            Err(ureq::Error::StatusCode(code)) if (400..500).contains(&code) => RobotsPolicy::Unavailable,
            Err(_) => RobotsPolicy::Unreachable,
        }
    }

    fn parse(body: &str, product_token: &str) -> Self {
        let mut rules = Vec::new();
        let mut in_matching_agent = false;
        for line in body.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some(agent) = strip_directive(line, "user-agent") {
                let agent = agent.trim();
                in_matching_agent = agent == "*" || agent.eq_ignore_ascii_case(product_token);
            } else if in_matching_agent {
                let rule = strip_directive(line, "disallow")
                    .map(|p| (false, p.trim()))
                    .or_else(|| strip_directive(line, "allow").map(|p| (true, p.trim())))
                    .filter(|(_, path)| !path.is_empty());
                if let Some((is_allow, path)) = rule {
                    rules.push((is_allow, path.to_string()));
                }
            }
        }
        Self { rules }
    }

    fn is_allowed(&self, url: &Url) -> bool {
        let path = url.path();
        let mut best_len = 0;
        let mut allowed = true;
        for (is_allow, pattern) in &self.rules {
            let len = pattern_match_len(pattern, path);
            if len > 0 && (len > best_len || (len == best_len && *is_allow)) {
                best_len = len;
                allowed = *is_allow;
            }
        }
        allowed
    }
}

/// RFC 9309 pattern match with `*` wildcard and `$` end anchor.
fn pattern_match_len(pattern: &str, path: &str) -> usize {
    let path = path.as_bytes();
    let pattern = pattern.as_bytes();
    let pathlen = path.len();
    let mut pos = vec![0usize];

    for (i, &pat) in pattern.iter().enumerate() {
        if pat == b'$' && i + 1 == pattern.len() {
            return if pos.last().copied() == Some(pathlen) {
                pattern.len()
            } else {
                0
            };
        }
        if pat == b'*' {
            if let Some(&first) = pos.first() {
                pos = (first..=pathlen).collect();
            }
        } else {
            pos = pos
                .into_iter()
                .filter(|&p| p < pathlen && path[p] == pat)
                .map(|p| p + 1)
                .collect();
            if pos.is_empty() {
                return 0;
            }
        }
    }
    pattern.len()
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

/// Run a BFS crawl starting from `opts.seed`.
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
    fn normalize_removes_fragment() {
        let u = Url::parse("https://example.com/page#section").unwrap();
        assert_eq!(normalize_url(&u), "https://example.com/page");
    }

    #[test]
    fn same_site_subdomains() {
        let a = Url::parse("https://www.example.com/a").unwrap();
        let b = Url::parse("https://docs.example.com/b").unwrap();
        assert!(is_same_site(&a, &b));
    }

    #[test]
    fn same_site_different_domain() {
        let a = Url::parse("https://example.com/a").unwrap();
        let b = Url::parse("https://other.com/b").unwrap();
        assert!(!is_same_site(&a, &b));
    }

    #[test]
    fn same_site_co_uk() {
        let a = Url::parse("https://www.example.co.uk/a").unwrap();
        let b = Url::parse("https://shop.example.co.uk/b").unwrap();
        let c = Url::parse("https://other.co.uk/c").unwrap();
        assert!(is_same_site(&a, &b));
        assert!(!is_same_site(&a, &c));
    }

    #[test]
    fn same_site_ip_fallback() {
        let a = Url::parse("http://192.0.2.1/a").unwrap();
        let b = Url::parse("http://192.0.2.1/b").unwrap();
        let c = Url::parse("http://198.51.100.1/c").unwrap();
        assert!(is_same_site(&a, &b));
        assert!(!is_same_site(&a, &c));
    }

    #[test]
    fn registrable_domain_works() {
        let u = Url::parse("https://sub.example.com/path").unwrap();
        assert_eq!(registrable_domain(&u).as_deref(), Some("example.com"));
    }

    #[test]
    fn robots_parse_allow_disallow() {
        let rules = RobotsRules::parse("User-agent: *\nDisallow: /admin\nAllow: /admin/public\n", "servo-fetch");
        assert!(!rules.is_allowed(&Url::parse("https://x.com/admin/secret").unwrap()));
        assert!(rules.is_allowed(&Url::parse("https://x.com/admin/public/page").unwrap()));
        assert!(rules.is_allowed(&Url::parse("https://x.com/page").unwrap()));
    }

    #[test]
    fn robots_longest_match_wins() {
        let rules = RobotsRules::parse(
            "User-agent: *\nAllow: /\nDisallow: /private\nAllow: /private/ok\n",
            "servo-fetch",
        );
        assert!(rules.is_allowed(&Url::parse("https://x.com/public").unwrap()));
        assert!(!rules.is_allowed(&Url::parse("https://x.com/private/secret").unwrap()));
        assert!(rules.is_allowed(&Url::parse("https://x.com/private/ok/page").unwrap()));
    }

    #[test]
    fn robots_empty_allows_all() {
        let rules = RobotsRules::parse("", "servo-fetch");
        assert!(rules.is_allowed(&Url::parse("https://x.com/anything").unwrap()));
    }

    #[test]
    fn robots_case_insensitive_directives() {
        let rules = RobotsRules::parse("USER-AGENT: *\nDISALLOW: /blocked\nALLOW: /blocked/ok\n", "servo-fetch");
        assert!(!rules.is_allowed(&Url::parse("https://x.com/blocked/secret").unwrap()));
        assert!(rules.is_allowed(&Url::parse("https://x.com/blocked/ok").unwrap()));
    }

    #[test]
    fn robots_wildcard() {
        let rules = RobotsRules::parse("User-agent: *\nDisallow: /private/*/secret\n", "servo-fetch");
        assert!(!rules.is_allowed(&Url::parse("https://x.com/private/foo/secret").unwrap()));
        assert!(!rules.is_allowed(&Url::parse("https://x.com/private/bar/baz/secret").unwrap()));
        assert!(rules.is_allowed(&Url::parse("https://x.com/private/foo/public").unwrap()));
    }

    #[test]
    fn robots_dollar_anchor() {
        let rules = RobotsRules::parse("User-agent: *\nDisallow: /*.pdf$\n", "servo-fetch");
        assert!(!rules.is_allowed(&Url::parse("https://x.com/doc/report.pdf").unwrap()));
        assert!(rules.is_allowed(&Url::parse("https://x.com/doc/report.pdf/view").unwrap()));
        assert!(rules.is_allowed(&Url::parse("https://x.com/doc/report.html").unwrap()));
    }

    #[test]
    fn pattern_match_google_compat() {
        assert!(pattern_match_len("/", "/") > 0);
        assert!(pattern_match_len("/fish", "/fish") > 0);
        assert!(pattern_match_len("/fish", "/fish.html") > 0);
        assert!(pattern_match_len("/fish*", "/fish") > 0);
        assert!(pattern_match_len("/fish*", "/fishheads") > 0);
        assert!(pattern_match_len("/fish*", "/fishheads/yummy.html") > 0);
        assert_eq!(pattern_match_len("/fish*", "/Fish.asp"), 0);
        assert_eq!(pattern_match_len("/fish", "/catfish"), 0);
        assert_eq!(pattern_match_len("/fish", "/"), 0);
        assert!(pattern_match_len("/*.php", "/index.php") > 0);
        assert!(pattern_match_len("/*.php$", "/filename.php") > 0);
        assert_eq!(pattern_match_len("/*.php$", "/filename.php/"), 0);
        assert_eq!(pattern_match_len("/*.php$", "/filename.php?a=1"), 0);
        assert!(pattern_match_len("/fish*.php", "/fish.php") > 0);
        assert!(pattern_match_len("/fish*.php", "/fishheads/catfish.php") > 0);
    }

    #[test]
    fn scope_include_exclude() {
        let inc = build_globset(&["/docs/**".into()]).ok();
        let exc = build_globset(&["/docs/archive/**".into()]).ok();
        let yes = Url::parse("https://example.com/docs/guide").unwrap();
        let no_exc = Url::parse("https://example.com/docs/archive/old").unwrap();
        let no_inc = Url::parse("https://example.com/blog/post").unwrap();
        assert!(matches_scope(&yes, inc.as_ref(), exc.as_ref()));
        assert!(!matches_scope(&no_exc, inc.as_ref(), exc.as_ref()));
        assert!(!matches_scope(&no_inc, inc.as_ref(), exc.as_ref()));
    }

    #[test]
    fn scope_no_filters() {
        assert!(matches_scope(
            &Url::parse("https://example.com/anything").unwrap(),
            None,
            None
        ));
    }

    #[test]
    fn scope_exclude_only() {
        let exc = build_globset(&["/secret/**".into()]).ok();
        assert!(matches_scope(
            &Url::parse("https://example.com/public").unwrap(),
            None,
            exc.as_ref()
        ));
        assert!(!matches_scope(
            &Url::parse("https://example.com/secret/data").unwrap(),
            None,
            exc.as_ref()
        ));
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

    #[test]
    fn robots_url_preserves_authority_and_drops_userinfo() {
        let cases = [
            (
                "http://u:p@example.com:8080/x?q=1#f",
                "http://example.com:8080/robots.txt",
            ),
            ("http://example.com:80/x", "http://example.com/robots.txt"),
            ("https://example.com:80/x", "https://example.com:80/robots.txt"),
            ("https://[2001:db8::1]:8443/", "https://[2001:db8::1]:8443/robots.txt"),
        ];
        for (input, expected) in cases {
            let seed = Url::parse(input).unwrap();
            assert_eq!(robots_url(&seed).unwrap().as_str(), expected, "input: {input}");
        }
    }

    #[test]
    fn product_token_extracts_leading_identifier() {
        assert_eq!(product_token("MyBot/1.0"), "MyBot");
        assert_eq!(product_token("MyBot/1.0 (+https://example.com)"), "MyBot");
        assert_eq!(product_token("servo-fetch/0.7.1"), "servo-fetch");
    }

    #[test]
    fn product_token_falls_back_to_wildcard() {
        assert_eq!(product_token(""), "*");
        assert_eq!(product_token("/MyBot"), "*");
        assert_eq!(product_token("   "), "*");
    }

    #[test]
    fn policy_unavailable_allows_all() {
        assert!(RobotsPolicy::Unavailable.is_allowed(&Url::parse("https://x.com/anything").unwrap()));
    }

    #[test]
    fn policy_unreachable_disallows_all() {
        assert!(!RobotsPolicy::Unreachable.is_allowed(&Url::parse("https://x.com/anything").unwrap()));
    }

    #[test]
    fn parse_honors_custom_product_token() {
        let body = "User-agent: MyBot\nDisallow: /private\nUser-agent: *\nAllow: /\n";
        let rules = RobotsRules::parse(body, "MyBot");
        assert!(!rules.is_allowed(&Url::parse("https://x.com/private").unwrap()));
        assert!(rules.is_allowed(&Url::parse("https://x.com/public").unwrap()));
    }

    #[test]
    fn parse_falls_back_to_wildcard_for_unknown_token() {
        let body = "User-agent: GoogleBot\nDisallow: /google-only\nUser-agent: *\nDisallow: /shared\n";
        let rules = RobotsRules::parse(body, "MyBot");
        assert!(rules.is_allowed(&Url::parse("https://x.com/google-only").unwrap()));
        assert!(!rules.is_allowed(&Url::parse("https://x.com/shared").unwrap()));
    }

    mod fetch {
        //! HTTP-level tests for `RobotsRules::fetch`
        use super::*;
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        async fn serve(status: u16, body: &str) -> (MockServer, Url) {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/robots.txt"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .mount(&server)
                .await;
            let seed = Url::parse(&server.uri()).unwrap();
            (server, seed)
        }

        async fn call(seed: Url, user_agent: Option<&'static str>) -> RobotsPolicy {
            tokio::task::spawn_blocking(move || RobotsRules::fetch(&seed, user_agent))
                .await
                .unwrap()
        }

        #[tokio::test]
        async fn ok_parses_rules_for_product_token() {
            let (_server, seed) = serve(200, "User-agent: MyBot\nDisallow: /private\n").await;
            let policy = call(seed.clone(), Some("MyBot/1.0")).await;
            let target = seed.join("/private").unwrap();
            assert!(!policy.is_allowed(&target));
            assert!(policy.is_allowed(&seed.join("/public").unwrap()));
        }

        #[tokio::test]
        async fn status_404_is_unavailable() {
            let (_server, seed) = serve(404, "").await;
            assert!(matches!(call(seed, None).await, RobotsPolicy::Unavailable));
        }

        #[tokio::test]
        async fn status_410_is_unavailable() {
            let (_server, seed) = serve(410, "").await;
            assert!(matches!(call(seed, None).await, RobotsPolicy::Unavailable));
        }

        #[tokio::test]
        async fn status_401_is_unreachable() {
            let (_server, seed) = serve(401, "").await;
            assert!(matches!(call(seed, None).await, RobotsPolicy::Unreachable));
        }

        #[tokio::test]
        async fn status_403_is_unreachable() {
            let (_server, seed) = serve(403, "").await;
            assert!(matches!(call(seed, None).await, RobotsPolicy::Unreachable));
        }

        #[tokio::test]
        async fn status_429_is_unreachable() {
            let (_server, seed) = serve(429, "").await;
            assert!(matches!(call(seed, None).await, RobotsPolicy::Unreachable));
        }

        #[tokio::test]
        async fn status_500_is_unreachable() {
            let (_server, seed) = serve(500, "").await;
            assert!(matches!(call(seed, None).await, RobotsPolicy::Unreachable));
        }

        #[tokio::test]
        async fn body_exceeding_size_limit_is_unreachable() {
            // ROBOTS_MAX_BYTES is 512 KiB; send slightly more.
            let size = usize::try_from(ROBOTS_MAX_BYTES).unwrap() + 1024;
            let oversized = "a".repeat(size);
            let (_server, seed) = serve(200, &oversized).await;
            assert!(matches!(call(seed, None).await, RobotsPolicy::Unreachable));
        }

        #[tokio::test]
        async fn sends_caller_provided_user_agent() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/robots.txt"))
                .and(header("user-agent", "CustomBot/9.9"))
                .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\n"))
                .expect(1)
                .mount(&server)
                .await;
            let seed = Url::parse(&server.uri()).unwrap();
            let _ = call(seed, Some("CustomBot/9.9")).await;
            // MockServer verifies `.expect(1)` on drop; a mismatching UA would fail.
        }
    }
}
