//! Load a Netscape `cookies.txt` file and seed Servo's cookie jar before navigation.

use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use url::Url;

use crate::error::{Error, Result};

const MAX_FILE_BYTES: u64 = 4 << 20;
const MAX_COOKIES: usize = 3000;

/// A cookie to seed into the jar before navigation.
#[derive(Clone, PartialEq, Eq)]
pub struct CookieSpec {
    name: String,
    value: String,
    domain: String,
    path: String,
    secure: bool,
    http_only: bool,
    include_subdomains: bool,
}

impl std::fmt::Debug for CookieSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieSpec")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("domain", &self.domain)
            .field("path", &self.path)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("include_subdomains", &self.include_subdomains)
            .finish()
    }
}

/// Load cookies from a Netscape/Mozilla `cookies.txt` file (curl/wget compatible).
pub fn load_cookies(path: impl AsRef<Path>) -> Result<Vec<CookieSpec>> {
    let path = path.as_ref();
    let fail = |reason: String| Error::Cookies {
        path: path.display().to_string(),
        reason,
    };
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_string(&mut text))
        .map_err(|e| fail(e.to_string()))?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(fail(format!("file exceeds {MAX_FILE_BYTES} bytes")));
    }
    parse_cookies(&text).map_err(|e| fail(e.to_string()))
}

#[derive(Debug, thiserror::Error)]
enum ParseError {
    #[error("line {line}: expected 7 tab-separated fields, found {found}")]
    FieldCount { line: usize, found: usize },
    #[error("line {line}: illegal character in cookie name or value")]
    IllegalChar { line: usize },
    #[error("too many cookies (max {max})")]
    TooMany { max: usize },
}

fn parse_cookies(text: &str) -> std::result::Result<Vec<CookieSpec>, ParseError> {
    let now = now_unix();
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let (http_only, rest) = match raw.strip_prefix("#HttpOnly_") {
            Some(rest) => (true, rest),
            None if raw.trim().is_empty() || raw.starts_with('#') => continue,
            None => (false, raw),
        };
        let fields: Vec<&str> = rest.split('\t').collect();
        let [domain, include_sub, cpath, secure, expires, name, value] = fields[..] else {
            return Err(ParseError::FieldCount {
                line,
                found: fields.len(),
            });
        };
        if has_control(name) || name.contains([';', '=']) || has_control(value) || value.contains(';') {
            return Err(ParseError::IllegalChar { line });
        }
        if expires
            .split('.')
            .next()
            .and_then(|s| s.trim().parse::<i64>().ok())
            .is_some_and(|e| e > 0 && e <= now)
        {
            continue;
        }
        if out.len() >= MAX_COOKIES {
            return Err(ParseError::TooMany { max: MAX_COOKIES });
        }
        out.push(CookieSpec {
            name: name.to_owned(),
            value: value.to_owned(),
            domain: domain.to_owned(),
            path: if cpath.is_empty() { "/" } else { cpath }.to_owned(),
            secure: secure.eq_ignore_ascii_case("TRUE"),
            http_only,
            include_subdomains: include_sub.eq_ignore_ascii_case("TRUE"),
        });
    }
    Ok(out)
}

/// Seed `specs` into the jar, scoped to `target`'s site and the network policy.
pub(crate) fn seed(servo: &servo::Servo, target: &Url, specs: &[CookieSpec]) {
    if specs.is_empty() {
        return;
    }
    let policy = crate::bridge::engine_policy();
    let manager = servo.site_data_manager();
    for spec in specs {
        if let Some((url, cookie)) = cookie_for(target, spec, policy) {
            manager.set_cookie_for_url(url, cookie);
        }
    }
}

/// Build a jar entry keyed by the cookie's own origin, or `None` if it is out of
/// `target`'s site or disallowed by `policy`.
fn cookie_for(
    target: &Url,
    spec: &CookieSpec,
    policy: crate::net::NetworkPolicy,
) -> Option<(Url, cookie::Cookie<'static>)> {
    let host = spec.domain.trim_start_matches('.');
    let scheme = if spec.secure { "https" } else { "http" };
    let url = Url::parse(&format!("{scheme}://{host}{}", spec.path)).ok()?;
    if crate::net::validate_url_with_policy(url.as_str(), policy).is_err() || !crate::scope::is_same_site(target, &url)
    {
        tracing::warn!(domain = %host, "skipped out-of-scope or disallowed cookie");
        return None;
    }
    let mut builder = cookie::Cookie::build((spec.name.clone(), spec.value.clone()))
        .path(spec.path.clone())
        .secure(spec.secure)
        .http_only(spec.http_only);
    if spec.domain.starts_with('.') || spec.include_subdomains {
        builder = builder.domain(url.host_str().unwrap_or(host).to_owned());
    }
    Some((url, builder.build()))
}

fn has_control(s: &str) -> bool {
    s.bytes().any(|b| b < 0x20 || b == 0x7f)
}

fn now_unix() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    i64::try_from(secs).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::net::NetworkPolicy;

    fn spec(domain: &str, secure: bool) -> CookieSpec {
        CookieSpec {
            name: "n".into(),
            value: "v".into(),
            domain: domain.into(),
            path: "/".into(),
            secure,
            http_only: false,
            include_subdomains: false,
        }
    }

    #[test]
    fn parses_standard_line() {
        let specs = parse_cookies(".example.com\tTRUE\t/\tTRUE\t0\tsid\tabc123\n").unwrap();
        assert_eq!(specs.len(), 1);
        let c = &specs[0];
        assert_eq!((c.name.as_str(), c.value.as_str()), ("sid", "abc123"));
        assert_eq!(c.domain, ".example.com");
        assert!(c.secure && c.include_subdomains && !c.http_only);
    }

    #[test]
    fn handles_httponly_prefix_and_comments() {
        let specs = parse_cookies("# a comment\n\n#HttpOnly_app.example.com\tFALSE\t/\tFALSE\t0\ttok\tv\n").unwrap();
        assert_eq!(specs.len(), 1);
        assert!(specs[0].http_only);
        assert_eq!(specs[0].domain, "app.example.com");
    }

    #[test]
    fn drops_expired_keeps_session() {
        // integer- and float-formatted past timestamps are dropped; 0 = session is kept.
        let specs = parse_cookies(
            "x.com\tFALSE\t/\tFALSE\t100\told\tv\nx.com\tFALSE\t/\tFALSE\t1700000000.5\tfloat\tv\nx.com\tFALSE\t/\tFALSE\t0\tlive\tv\n",
        )
        .unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "live");
    }

    #[test]
    fn empty_path_defaults_to_root() {
        assert_eq!(parse_cookies("x.com\tFALSE\t\tFALSE\t0\tn\tv\n").unwrap()[0].path, "/");
    }

    #[test]
    fn rejects_wrong_field_count() {
        assert!(parse_cookies("x.com\tFALSE\t/\tFALSE\t0\tn\n").is_err());
    }

    #[test]
    fn rejects_illegal_chars() {
        assert!(parse_cookies("x.com\tFALSE\t/\tFALSE\t0\tn\ta;b\n").is_err());
        assert!(parse_cookies("x.com\tFALSE\t/\tFALSE\t0\tn=x\tv\n").is_err());
        // '=' is legal in a value (e.g. base64 padding).
        assert!(parse_cookies("x.com\tFALSE\t/\tFALSE\t0\tn\tYWJj==\n").is_ok());
        // errors never echo the offending value.
        let err = parse_cookies("x.com\tFALSE\t/\tFALSE\t0\tn\tval\rinjected\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("illegal character") && !err.contains("injected"));
    }

    #[test]
    fn load_reads_and_parses_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b".example.com\tTRUE\t/\tFALSE\t0\tn\tv\n").unwrap();
        let specs = load_cookies(f.path()).unwrap();
        assert_eq!(
            specs,
            vec![CookieSpec {
                name: "n".to_owned(),
                value: "v".to_owned(),
                domain: ".example.com".to_owned(),
                path: "/".to_owned(),
                secure: false,
                http_only: false,
                include_subdomains: true,
            }]
        );
    }

    #[test]
    fn missing_file_reports_path() {
        let err = load_cookies("/no/such/cookies.txt").unwrap_err();
        assert!(matches!(err, Error::Cookies { .. }));
    }

    #[test]
    fn debug_redacts_value() {
        let mut c = spec("example.com", false);
        c.value = "SUPERSECRET".into();
        let dbg = format!("{c:?}");
        assert!(dbg.contains("<redacted>") && !dbg.contains("SUPERSECRET"));
    }

    #[test]
    fn rejects_too_many_cookies() {
        let text = "x.com\tFALSE\t/\tFALSE\t0\tn\tv\n".repeat(MAX_COOKIES + 1);
        assert!(matches!(parse_cookies(&text), Err(ParseError::TooMany { .. })));
    }

    #[test]
    fn cookie_for_scopes_to_same_site() {
        let target = Url::parse("https://example.com/").unwrap();
        assert!(cookie_for(&target, &spec("app.example.com", false), NetworkPolicy::STRICT).is_some());
        assert!(cookie_for(&target, &spec("evil.com", false), NetworkPolicy::STRICT).is_none());
    }

    #[test]
    fn cookie_for_derives_origin_and_domain_attr() {
        let target = Url::parse("https://example.com/").unwrap();
        // host-only over https: secure flag picks the https origin, no Domain attribute.
        let (url, c) = cookie_for(&target, &spec("example.com", true), NetworkPolicy::STRICT).unwrap();
        assert_eq!(url.scheme(), "https");
        assert!(c.domain().is_none());
        // leading dot makes it a domain cookie scoped to the registrable host.
        let (_, c) = cookie_for(&target, &spec(".example.com", false), NetworkPolicy::STRICT).unwrap();
        assert_eq!(c.domain(), Some("example.com"));
    }

    #[test]
    fn cookie_for_blocks_private_under_strict_only() {
        let target = Url::parse("http://127.0.0.1/").unwrap();
        assert!(cookie_for(&target, &spec("127.0.0.1", false), NetworkPolicy::STRICT).is_none());
        assert!(cookie_for(&target, &spec("127.0.0.1", false), NetworkPolicy::PERMISSIVE).is_some());
    }
}
