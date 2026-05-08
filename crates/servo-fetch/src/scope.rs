//! URL scope utilities — normalization, same-site checks, and glob-based filtering.

use globset::{Glob, GlobSet, GlobSetBuilder};
use url::Url;

pub(crate) fn normalize_url(url: &Url) -> String {
    let mut u = url.clone();
    u.set_fragment(None);
    u.to_string()
}

pub(crate) fn is_same_site(seed: &Url, candidate: &Url) -> bool {
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

pub(crate) fn matches_scope(url: &Url, include: Option<&GlobSet>, exclude: Option<&GlobSet>) -> bool {
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
pub(crate) fn build_globset(patterns: &[String]) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p)?);
    }
    builder.build()
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
}
