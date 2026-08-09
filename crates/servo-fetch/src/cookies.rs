//! Load a Netscape `cookies.txt` file and seed Servo's cookie jar before navigation.

use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use url::{Host, Url};

use crate::error::{Error, Result};

const MAX_FILE_BYTES: u64 = 4 << 20;
const MAX_COOKIES: usize = 3000;
const MAX_COOKIE_NAME_VALUE_BYTES: usize = 4096;
const MAX_COOKIE_HEADER_BYTES: usize = 8190;
const COOKIE_HEADER_PREFIX_BYTES: usize = "Cookie: ".len();

/// A cookie to seed into the jar before navigation.
#[derive(Clone, PartialEq, Eq)]
pub struct CookieSpec {
    name: String,
    value: String,
    domain: String,
    path: String,
    expires: Option<i64>,
    secure: bool,
    http_only: bool,
    include_subdomains: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct CookieWire {
    name: String,
    value: String,
    domain: String,
    path: String,
    expires: Option<i64>,
    secure: bool,
    http_only: bool,
    include_subdomains: bool,
}
impl CookieWire {
    pub(crate) fn from_specs(specs: &[CookieSpec]) -> Vec<Self> {
        specs
            .iter()
            .map(|spec| Self {
                name: spec.name.clone(),
                value: spec.value.clone(),
                domain: spec.domain.clone(),
                path: spec.path.clone(),
                secure: spec.secure,
                http_only: spec.http_only,
                include_subdomains: spec.include_subdomains,
                expires: spec.expires,
            })
            .collect()
    }

    pub(crate) fn into_specs(wires: Vec<Self>) -> std::result::Result<Vec<CookieSpec>, &'static str> {
        if wires.len() > MAX_COOKIES {
            return Err("too many cookies in worker request");
        }
        wires
            .into_iter()
            .map(|w| {
                if w.name.is_empty()
                    || has_control(&w.name)
                    || w.name.contains([';', '='])
                    || has_control(&w.value)
                    || w.value.contains(';')
                    || w.name.len().saturating_add(w.value.len()) > MAX_COOKIE_NAME_VALUE_BYTES
                    || w.domain.len() > 253
                    || has_control(&w.domain)
                    || canonical_cookie_host(&w.domain).is_none()
                    || !w.path.starts_with('/')
                    || w.path.len() > 4096
                    || has_control(&w.path)
                {
                    return Err("invalid cookie in worker request");
                }
                Ok(CookieSpec {
                    name: w.name,
                    value: w.value,
                    domain: w.domain,
                    path: w.path,
                    expires: w.expires,
                    secure: w.secure,
                    http_only: w.http_only,
                    include_subdomains: w.include_subdomains,
                })
            })
            .collect()
    }
}

impl std::fmt::Debug for CookieSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieSpec")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("domain", &self.domain)
            .field("path", &self.path)
            .field("expires", &self.expires)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("include_subdomains", &self.include_subdomains)
            .finish()
    }
}

/// Load cookies from a Netscape format `cookies.txt` file.
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
    #[error("line {line}: cookie name and value exceed {max} bytes")]
    TooLarge { line: usize, max: usize },
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
        if name.is_empty()
            || has_control(name)
            || name.contains([';', '='])
            || has_control(value)
            || value.contains(';')
        {
            return Err(ParseError::IllegalChar { line });
        }
        if name.len().saturating_add(value.len()) > MAX_COOKIE_NAME_VALUE_BYTES {
            return Err(ParseError::TooLarge {
                line,
                max: MAX_COOKIE_NAME_VALUE_BYTES,
            });
        }

        let path = if cpath.starts_with('/') { cpath } else { "/" };
        let include_subdomains = include_sub.eq_ignore_ascii_case("TRUE");
        let existing = out
            .iter()
            .position(|cookie| same_cookie_key(cookie, name, domain, path));
        let expires_at = match expires
            .split('.')
            .next()
            .and_then(|value| value.trim().parse::<i64>().ok())
        {
            Some(0) => None,
            Some(expiry) if expiry > 0 => Some(expiry),
            _ => continue,
        };
        if expires_at.is_some_and(|expiry| expiry <= now) {
            if let Some(index) = existing {
                out.remove(index);
            }
            continue;
        }

        let cookie = CookieSpec {
            name: name.to_owned(),
            value: value.to_owned(),
            domain: domain.to_owned(),
            path: path.to_owned(),
            expires: expires_at,
            secure: secure.eq_ignore_ascii_case("TRUE"),
            http_only,
            include_subdomains,
        };
        if let Some(index) = existing {
            out[index] = cookie;
        } else {
            if out.len() >= MAX_COOKIES {
                return Err(ParseError::TooMany { max: MAX_COOKIES });
            }
            out.push(cookie);
        }
    }
    Ok(out)
}

fn canonical_cookie_host(domain: &str) -> Option<Host<String>> {
    let domain = domain.strip_prefix('.').unwrap_or(domain);
    if domain.contains(':') && !domain.starts_with('[') {
        Host::parse(&format!("[{domain}]")).ok()
    } else {
        Host::parse(domain).ok()
    }
}

fn same_cookie_key(cookie: &CookieSpec, name: &str, domain: &str, path: &str) -> bool {
    let domains_equal = match (canonical_cookie_host(&cookie.domain), canonical_cookie_host(domain)) {
        (Some(left), Some(right)) => left == right,
        _ => cookie
            .domain
            .strip_prefix('.')
            .unwrap_or(&cookie.domain)
            .eq_ignore_ascii_case(domain.strip_prefix('.').unwrap_or(domain)),
    };
    cookie.name == name && domains_equal && cookie.path == path
}

fn domain_matches(target: &Url, spec: &CookieSpec) -> bool {
    match (target.host(), canonical_cookie_host(&spec.domain)) {
        (Some(Host::Domain(request)), Some(Host::Domain(cookie))) => {
            request == cookie
                || (spec.include_subdomains
                    && request
                        .strip_suffix(&cookie)
                        .is_some_and(|prefix| prefix.ends_with('.')))
        }
        (Some(Host::Ipv4(request)), Some(Host::Ipv4(cookie))) => request == cookie,
        (Some(Host::Ipv6(request)), Some(Host::Ipv6(cookie))) => request == cookie,
        _ => false,
    }
}

fn is_secure_context(target: &Url) -> bool {
    target.scheme() == "https"
        || match target.host() {
            Some(Host::Domain(domain)) => domain == "localhost",
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            None => false,
        }
}

fn cookie_is_expired(cookie: &CookieSpec, now: i64) -> bool {
    cookie.expires.is_some_and(|expiry| expiry <= now)
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
            manager.set_cookie_for_url(url, cookie, None);
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
    if cookie_is_expired(spec, now_unix()) {
        return None;
    }
    let host = canonical_cookie_host(&spec.domain)?;
    let host = host.to_string();
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
    if let Some(expires) = spec.expires
        && let Ok(expires) = cookie::time::OffsetDateTime::from_unix_timestamp(expires)
    {
        builder = builder.expires(expires);
    }
    if spec.include_subdomains {
        builder = builder.domain(url.host_str().unwrap_or(&host).to_owned());
    }
    Some((url, builder.build()))
}

pub(crate) fn request_header(target: &Url, specs: &[CookieSpec]) -> Option<http::HeaderValue> {
    let request_path = target.path();
    let secure = is_secure_context(target);
    let now = now_unix();
    let mut matches = specs
        .iter()
        .filter(|spec| {
            !cookie_is_expired(spec, now)
                && domain_matches(target, spec)
                && (!spec.secure || secure)
                && path_matches(request_path, &spec.path)
        })
        .collect::<Vec<_>>();
    // RFC 6265 section 5.4: longer paths first. `sort_by_key` is stable, so
    // equal-length paths preserve the file order used as creation order.
    matches.sort_by_key(|spec| std::cmp::Reverse(spec.path.len()));

    let mut value = String::new();
    for spec in matches {
        let separator_len = usize::from(!value.is_empty()) * 2;
        let pair_len = spec.name.len().saturating_add(1).saturating_add(spec.value.len());
        if COOKIE_HEADER_PREFIX_BYTES
            .saturating_add(value.len())
            .saturating_add(separator_len)
            .saturating_add(pair_len)
            >= MAX_COOKIE_HEADER_BYTES
        {
            tracing::warn!(name = %spec.name, "omitted cookies after reaching outgoing header limit");
            break;
        }
        if !value.is_empty() {
            value.push_str("; ");
        }
        value.push_str(&spec.name);
        value.push('=');
        value.push_str(&spec.value);
    }
    (!value.is_empty())
        .then(|| http::HeaderValue::from_str(&value).ok())
        .flatten()
}

fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    request_path == cookie_path
        || request_path
            .strip_prefix(cookie_path)
            .is_some_and(|suffix| cookie_path.ends_with('/') || suffix.starts_with('/'))
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
    use std::fmt::Write as _;
    use std::io::Write as _;

    use super::*;
    use crate::net::NetworkPolicy;

    fn spec(domain: &str, secure: bool) -> CookieSpec {
        CookieSpec {
            name: "n".into(),
            value: "v".into(),
            domain: domain.into(),
            path: "/".into(),
            expires: None,
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
        // Past, negative, and malformed timestamps are dropped; 0 is a session cookie.
        let specs = parse_cookies(
            "x.com\tFALSE\t/\tFALSE\t100\told\tv\nx.com\tFALSE\t/\tFALSE\t1700000000.5\tfloat\tv\nx.com\tFALSE\t/\tFALSE\t-1\tnegative\tv\nx.com\tFALSE\t/\tFALSE\tinvalid\tmalformed\tv\nx.com\tFALSE\t/\tFALSE\t0\tlive\tv\n",
        )
        .unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "live");
    }

    #[test]
    fn duplicate_cookie_replaces_value_and_expiry_deletes() {
        let replaced = parse_cookies(concat!(
            ".example.com\tTRUE\t/\tFALSE\t0\tsid\told\n",
            "example.com\tFALSE\t/\tTRUE\t0\tsid\tnew\n",
        ))
        .unwrap();
        assert_eq!(replaced.len(), 1);
        assert_eq!(replaced[0].value, "new");
        assert!(replaced[0].secure);
        assert!(!replaced[0].include_subdomains);

        let deleted = parse_cookies(concat!(
            "example.com\tFALSE\t/\tFALSE\t0\tsid\tlive\n",
            "example.com\tFALSE\t/\tFALSE\t1\tsid\tdeleted\n",
        ))
        .unwrap();
        assert!(deleted.is_empty());
    }

    #[test]
    fn expiry_is_preserved_and_rechecked() {
        let future = now_unix() + 60;
        let parsed = parse_cookies(&format!("example.com\tFALSE\t/\tFALSE\t{future}\tsid\tlive\n")).unwrap();
        assert_eq!(parsed[0].expires, Some(future));

        let mut expired = spec("example.com", false);
        expired.expires = Some(1);
        let target = Url::parse("https://example.com/report.pdf").unwrap();
        assert!(request_header(&target, std::slice::from_ref(&expired)).is_none());
        assert!(cookie_for(&target, &expired, NetworkPolicy::STRICT).is_none());
    }

    #[test]
    fn rejects_oversized_cookie() {
        let input = format!(
            "example.com\tFALSE\t/\tFALSE\t0\tn\t{}\n",
            "x".repeat(MAX_COOKIE_NAME_VALUE_BYTES)
        );
        assert!(matches!(parse_cookies(&input), Err(ParseError::TooLarge { .. })));
    }

    #[test]
    fn invalid_or_empty_path_defaults_to_root() {
        assert_eq!(parse_cookies("x.com\tFALSE\t\tFALSE\t0\tn\tv\n").unwrap()[0].path, "/");
        assert_eq!(
            parse_cookies("x.com\tFALSE\taccount\tFALSE\t0\tn\tv\n").unwrap()[0].path,
            "/"
        );
    }

    #[test]
    fn rejects_wrong_field_count() {
        assert!(parse_cookies("x.com\tFALSE\t/\tFALSE\t0\tn\n").is_err());
    }

    #[test]
    fn rejects_illegal_chars() {
        assert!(parse_cookies("x.com\tFALSE\t/\tFALSE\t0\t\tv\n").is_err());
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
                expires: None,
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
    fn request_header_applies_cookie_retrieval_rules() {
        let mut host_only = spec("api.example.com", false);
        host_only.name = "host".into();
        let mut domain = spec(".example.com", false);
        domain.name = "domain".into();
        domain.include_subdomains = true;
        domain.path = "/public/deep".into();
        let mut path = spec("www.example.com", false);
        path.name = "path".into();
        path.path = "/admin".into();
        let mut secure = spec("www.example.com", true);
        secure.name = "secure".into();
        let target = Url::parse("http://www.example.com/public/deep/report.pdf").unwrap();

        let header = request_header(&target, &[host_only, domain, path, secure])
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(header, "domain=v");
    }

    #[test]
    fn request_header_respects_tailmatch_and_ip_rules() {
        let target = Url::parse("https://www.example.com/report.pdf").unwrap();
        let mut cookie = spec(".example.com", false);
        assert!(request_header(&target, std::slice::from_ref(&cookie)).is_none());
        cookie.include_subdomains = true;
        assert_eq!(request_header(&target, &[cookie]).unwrap().to_str().unwrap(), "n=v");

        let target = Url::parse("http://127.0.0.1/report.pdf").unwrap();
        let mut suffix = spec("0.0.1", false);
        suffix.include_subdomains = true;
        assert!(request_header(&target, &[suffix]).is_none());
        assert_eq!(
            request_header(&target, &[spec("127.0.0.1", false)])
                .unwrap()
                .to_str()
                .unwrap(),
            "n=v"
        );
    }

    #[test]
    fn request_header_canonicalizes_idna_and_allows_secure_loopback() {
        let target = Url::parse("http://xn--bcher-kva.example/report.pdf").unwrap();
        assert_eq!(
            request_header(&target, &[spec("bücher.example", false)])
                .unwrap()
                .to_str()
                .unwrap(),
            "n=v"
        );

        for url in [
            "http://localhost/report.pdf",
            "http://127.0.0.2/report.pdf",
            "http://[::1]/report.pdf",
        ] {
            let target = Url::parse(url).unwrap();
            let domain = target.host_str().unwrap();
            assert_eq!(
                request_header(&target, &[spec(domain, true)])
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "n=v"
            );
        }
    }

    #[test]
    fn request_header_orders_stably_and_caps_output() {
        let mut root = spec("example.com", false);
        root.name = "root".into();
        let mut first = spec("example.com", false);
        first.name = "first".into();
        first.path = "/account".into();
        let mut second = spec("example.com", false);
        second.name = "second".into();
        second.path = "/account".into();
        let target = Url::parse("https://example.com/account/report.pdf").unwrap();
        assert_eq!(
            request_header(&target, &[root, first, second])
                .unwrap()
                .to_str()
                .unwrap(),
            "first=v; second=v; root=v"
        );

        let mut first = spec("example.com", false);
        first.name = "a".into();
        first.value = "x".repeat(MAX_COOKIE_NAME_VALUE_BYTES - 1);
        let mut second = first.clone();
        second.name = "b".into();
        let header = request_header(&target, &[first, second]).unwrap();
        let value = header.to_str().unwrap();
        assert!(value.starts_with("a=") && !value.contains("; b="));
        assert!(COOKIE_HEADER_PREFIX_BYTES + value.len() < MAX_COOKIE_HEADER_BYTES);
    }

    #[test]
    fn cookie_wire_preserves_valid_fields_and_rejects_invalid_input() {
        let mut expected = spec("example.com", true);
        expected.path = "/account".into();
        expected.expires = Some(2_000_000_000);
        expected.http_only = true;
        let valid = CookieWire {
            name: expected.name.clone(),
            value: expected.value.clone(),
            domain: expected.domain.clone(),
            path: expected.path.clone(),
            expires: expected.expires,
            secure: expected.secure,
            http_only: expected.http_only,
            include_subdomains: expected.include_subdomains,
        };
        assert_eq!(CookieWire::into_specs(vec![valid]).unwrap(), [expected]);

        let invalid = CookieWire {
            name: "bad;name".into(),
            value: "secret".into(),
            domain: "example.com".into(),
            path: "/".into(),
            expires: None,
            secure: false,
            http_only: false,
            include_subdomains: false,
        };
        assert!(CookieWire::into_specs(vec![invalid]).is_err());
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
        let mut text = String::new();
        for index in 0..=MAX_COOKIES {
            writeln!(text, "x.com\tFALSE\t/\tFALSE\t0\tn{index}\tv").unwrap();
        }
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
        // Host-only over https: secure flag picks the https origin, no Domain attribute.
        let (url, c) = cookie_for(&target, &spec("example.com", true), NetworkPolicy::STRICT).unwrap();
        assert_eq!(url.scheme(), "https");
        assert!(c.domain().is_none());

        // The Netscape tailmatch column, not a decorative leading dot, controls
        // whether the cookie is a domain cookie.
        let (_, c) = cookie_for(&target, &spec(".example.com", false), NetworkPolicy::STRICT).unwrap();
        assert!(c.domain().is_none());
        let mut domain = spec(".example.com", false);
        domain.include_subdomains = true;
        let (_, c) = cookie_for(&target, &domain, NetworkPolicy::STRICT).unwrap();
        assert_eq!(c.domain(), Some("example.com"));
    }

    #[test]
    fn cookie_for_blocks_private_under_strict_only() {
        let target = Url::parse("http://127.0.0.1/").unwrap();
        assert!(cookie_for(&target, &spec("127.0.0.1", false), NetworkPolicy::STRICT).is_none());
        assert!(cookie_for(&target, &spec("127.0.0.1", false), NetworkPolicy::PERMISSIVE).is_some());

        let target = Url::parse("http://[::1]/").unwrap();
        let (url, _) = cookie_for(&target, &spec("::1", false), NetworkPolicy::PERMISSIVE).unwrap();
        assert_eq!(url.host(), Some(Host::Ipv6("::1".parse().unwrap())));
    }
}
