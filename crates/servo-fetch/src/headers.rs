//! Custom request-header parsing and validation, shared by every front-end.

use http::header::{HeaderMap, HeaderName, HeaderValue};

use crate::error::{Error, Result};

/// Hop-by-hop and message-framing headers the engine manages.
const RESERVED: &[&str] = &[
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "keep-alive",
];

/// Headers with a dedicated option; steer callers there instead of raw headers.
fn steer(name: &str) -> Option<&'static str> {
    match name {
        "user-agent" => Some("the user-agent option"),
        "cookie" => Some("the cookies option"),
        _ => None,
    }
}

/// Build a validated [`HeaderMap`] from `name`/`value` pairs (e.g. a JSON object).
pub fn from_pairs<I, K, V>(pairs: I) -> Result<HeaderMap>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        let (name, value) = validate(name.as_ref(), value.as_ref())?;
        headers.append(name, value);
    }
    Ok(headers)
}

/// Build a validated [`HeaderMap`] from curl-style `Name: Value` lines
/// (`Name;` sends an empty value).
pub fn parse_lines<I, S>(lines: I) -> Result<HeaderMap>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut headers = HeaderMap::new();
    for line in lines {
        let (name, value) = split_line(line.as_ref())?;
        let (name, value) = validate(&name, &value)?;
        headers.append(name, value);
    }
    Ok(headers)
}

fn validate(name: &str, value: &str) -> Result<(HeaderName, HeaderValue)> {
    let name = HeaderName::from_bytes(name.trim().as_bytes())
        .map_err(|_| Error::invalid_header(format!("invalid header name '{}'", name.trim())))?;
    if let Some(hint) = steer(name.as_str()) {
        return Err(Error::invalid_header(format!(
            "header '{name}' cannot be set as a custom header; use {hint} instead"
        )));
    }
    if RESERVED.contains(&name.as_str()) {
        return Err(Error::invalid_header(format!(
            "header '{name}' is managed by the engine and cannot be overridden"
        )));
    }
    let value = HeaderValue::from_str(value)
        .map_err(|_| Error::invalid_header(format!("invalid value for header '{name}'")))?;
    Ok((name, value))
}

fn split_line(line: &str) -> Result<(String, String)> {
    match line.split_once(':') {
        Some((name, value)) => {
            let value = value.trim_start();
            if value.is_empty() {
                return Err(malformed(line));
            }
            Ok((name.to_owned(), value.to_owned()))
        }
        None => match line.strip_suffix(';') {
            Some(name) if !name.is_empty() => Ok((name.to_owned(), String::new())),
            _ => Err(malformed(line)),
        },
    }
}

fn malformed(line: &str) -> Error {
    Error::invalid_header(format!(
        "invalid header '{line}': expected 'Name: Value' (use 'Name;' for an empty value)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_appends_lines() {
        let h = parse_lines(["X-A: 1", "X-B: two"]).unwrap();
        assert_eq!(h.get("x-a").unwrap(), "1", "first header");
        assert_eq!(h.get("x-b").unwrap(), "two", "second header");
    }

    #[test]
    fn empty_value_via_trailing_semicolon() {
        let h = parse_lines(["X-Empty;"]).unwrap();
        assert_eq!(h.get("x-empty").unwrap(), "", "semicolon sends empty value");
    }

    #[test]
    fn from_pairs_builds_map() {
        let h = from_pairs([("X-A", "1"), ("X-A", "2")]).unwrap();
        let values: Vec<_> = h.get_all("x-a").iter().collect();
        assert_eq!(values.len(), 2, "duplicate names are appended");
    }

    #[test]
    fn rejects_crlf_injection() {
        assert!(parse_lines(["X-A: a\r\nEvil: b"]).is_err(), "CRLF must be rejected");
        assert!(from_pairs([("X-A", "a\r\nEvil: b")]).is_err(), "CRLF must be rejected");
    }

    #[test]
    fn rejects_reserved_and_steered() {
        assert!(parse_lines(["Host: x"]).is_err(), "framing header rejected");
        assert!(parse_lines(["User-Agent: x"]).is_err(), "user-agent steered");
        assert!(parse_lines(["Cookie: a=1"]).is_err(), "cookie steered");
    }

    #[test]
    fn rejects_malformed_lines() {
        assert!(parse_lines(["no-colon"]).is_err(), "missing colon");
        assert!(parse_lines(["X-Foo:"]).is_err(), "empty value needs 'Name;'");
    }
}
