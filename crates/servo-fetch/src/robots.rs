//! robots.txt parsing and policy enforcement (RFC 9309).

use std::time::Duration;

use url::Url;

use crate::bridge;

pub(crate) const ROBOTS_MAX_BYTES: u64 = 512 * 1024;

/// Outcome of `RobotsRules::fetch`.
pub(crate) enum RobotsPolicy {
    Rules(RobotsRules),
    /// 4xx other than 401/403 — treat as no restrictions.
    Unavailable,
    /// Auth wall, server error, or network failure — fail closed.
    Unreachable,
}

impl RobotsPolicy {
    pub(crate) fn is_allowed(&self, url: &Url) -> bool {
        match self {
            Self::Rules(r) => r.is_allowed(url),
            Self::Unavailable => true,
            Self::Unreachable => false,
        }
    }
}

pub(crate) struct RobotsRules {
    pub(crate) rules: Vec<(bool, String)>,
    pub(crate) sitemaps: Vec<Url>,
}

impl RobotsRules {
    pub(crate) fn fetch(
        seed: &Url,
        user_agent: Option<&str>,
        headers: &http::HeaderMap,
        timeout: Duration,
    ) -> RobotsPolicy {
        let Some(url) = robots_url(seed) else {
            return RobotsPolicy::Unreachable;
        };
        let ua = user_agent.unwrap_or_else(|| bridge::default_user_agent());
        let agent = ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_global(Some(timeout))
                .user_agent(ua)
                .build(),
        );
        let mut req = agent.get(url.as_str());
        for (name, value) in headers {
            req = req.header(name.clone(), value.clone());
        }
        let Ok(resp) = req.call() else {
            return RobotsPolicy::Unreachable;
        };
        let status = resp.status().as_u16();
        let body = if (400..600).contains(&status) {
            None
        } else {
            resp.into_body()
                .with_config()
                .limit(ROBOTS_MAX_BYTES)
                .read_to_vec()
                .ok()
        };
        classify_response(status, body.as_deref(), product_token(ua))
    }

    fn parse(body: &str, product_token: &str) -> Self {
        let mut rules = Vec::new();
        let mut sitemaps = Vec::new();
        let mut in_matching_agent = false;
        for line in body.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some(val) = strip_directive(line, "sitemap") {
                if let Ok(url) = Url::parse(val.trim()) {
                    sitemaps.push(url);
                }
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
        Self { rules, sitemaps }
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

pub(crate) fn classify_response(status: u16, body: Option<&[u8]>, product_token: &str) -> RobotsPolicy {
    match status {
        // Stricter than RFC 9309/Google on auth-gated robots.txt: 401/403 read as "not welcome".
        401 | 403 | 429 | 500..=599 => RobotsPolicy::Unreachable,
        400..=499 => RobotsPolicy::Unavailable,
        _ => body
            .map(String::from_utf8_lossy)
            .map_or(RobotsPolicy::Unreachable, |body| {
                RobotsPolicy::Rules(RobotsRules::parse(&body, product_token))
            }),
    }
}

fn strip_directive<'a>(line: &'a str, directive: &str) -> Option<&'a str> {
    let len = directive.len();
    if line.len() > len && line[..len].eq_ignore_ascii_case(directive) && line.as_bytes()[len] == b':' {
        Some(&line[len + 1..])
    } else {
        None
    }
}

pub(crate) fn robots_url(seed: &Url) -> Option<Url> {
    let mut base = seed.clone();
    base.set_username("").ok();
    base.set_password(None).ok();
    base.join("/robots.txt").ok()
}

pub(crate) fn product_token(user_agent: &str) -> &str {
    user_agent
        .split(|c: char| c == '/' || c.is_whitespace())
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("*")
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

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

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
    fn classify_response_decodes_invalid_utf8_lossily() {
        let body = b"User-agent: *\nDisallow: /private\n# invalid byte: \xff\nAllow: /private/public\n";
        let policy = classify_response(200, Some(body), "servo-fetch");

        assert!(!policy.is_allowed(&Url::parse("https://x.com/private/secret").unwrap()));
        assert!(policy.is_allowed(&Url::parse("https://x.com/private/public/page").unwrap()));
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
        use super::*;

        async fn serve(status: u16, body: &str) -> (MockServer, Url) {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/robots.txt"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "text/plain; charset=utf-8"),
                )
                .mount(&server)
                .await;
            let seed = Url::parse(&server.uri()).unwrap();
            (server, seed)
        }

        async fn call(seed: Url, user_agent: Option<&'static str>) -> RobotsPolicy {
            tokio::task::spawn_blocking(move || {
                RobotsRules::fetch(&seed, user_agent, &http::HeaderMap::new(), Duration::from_secs(5))
            })
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
        async fn status_302_parses_rules() {
            let (_server, seed) = serve(302, "User-agent: *\nDisallow: /private\n").await;
            let policy = call(seed.clone(), None).await;
            assert!(!policy.is_allowed(&seed.join("/private").unwrap()));
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
                .respond_with(
                    ResponseTemplate::new(200).set_body_raw(b"User-agent: *\n".to_vec(), "text/plain; charset=utf-8"),
                )
                .expect(1)
                .mount(&server)
                .await;
            let seed = Url::parse(&server.uri()).unwrap();
            let _ = call(seed, Some("CustomBot/9.9")).await;
        }
    }
}
