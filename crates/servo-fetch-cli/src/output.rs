//! Output sinks.

use std::fs;
use std::io::Write as _;
use std::path::Path;

use anyhow::{Result, bail};

use servo_fetch::Page;

/// File extension for sink-emitted content.
#[derive(Debug, Copy, Clone)]
pub(crate) enum Ext {
    Markdown,
    Json,
    Html,
    Text,
}

impl Ext {
    fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
            Self::Html => "html",
            Self::Text => "txt",
        }
    }
}

impl std::fmt::Display for Ext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where rendered output goes.
#[derive(Debug, Copy, Clone)]
pub(crate) enum Sink<'a> {
    Stdout,
    File(&'a Path),
    Dir(&'a Path),
}

impl<'a> Sink<'a> {
    pub(crate) fn from_dir(dir: Option<&'a Path>) -> Self {
        dir.map_or(Self::Stdout, Self::Dir)
    }

    pub(crate) fn from_args(file: Option<&'a Path>, dir: Option<&'a Path>) -> Self {
        match (file, dir) {
            (Some(p), _) => Self::File(p),
            (_, Some(d)) => Self::Dir(d),
            _ => Self::Stdout,
        }
    }

    pub(crate) fn is_stdout(&self) -> bool {
        matches!(self, Self::Stdout)
    }

    pub(crate) fn write(&self, url: &str, ext: Ext, content: &str) -> Result<()> {
        self.emit(url, ext, content, false)
    }

    pub(crate) fn writeln(&self, url: &str, ext: Ext, content: &str) -> Result<()> {
        self.emit(url, ext, content, true)
    }

    fn emit(&self, url: &str, ext: Ext, content: &str, ensure_newline: bool) -> Result<()> {
        let sanitized = servo_fetch::sanitize::sanitize(content);
        let needs_nl = ensure_newline && !sanitized.ends_with('\n');
        match self {
            Self::Stdout => {
                let mut out = std::io::stdout().lock();
                out.write_all(sanitized.as_bytes())?;
                if needs_nl {
                    out.write_all(b"\n")?;
                }
                Ok(())
            }
            Self::File(path) => write_to_file(url, path, sanitized.as_bytes(), needs_nl),
            Self::Dir(dir) => {
                let path = dir.join(slug_from_url(url, ext));
                write_to_file(url, &path, sanitized.as_bytes(), needs_nl)
            }
        }
    }
}

fn write_to_file(url: &str, path: &Path, body: &[u8], with_newline: bool) -> Result<()> {
    let mut f = fs::File::create(path)?;
    f.write_all(body)?;
    if with_newline {
        f.write_all(b"\n")?;
    }
    tracing::info!(url = %url, path = %path.display(), bytes = body.len(), "saved");
    Ok(())
}

pub(crate) struct Markdown<'a> {
    pub page: &'a Page,
    pub url: &'a str,
    pub selector: Option<&'a str>,
}

impl Markdown<'_> {
    pub(crate) fn execute(&self, sink: Sink<'_>) -> Result<()> {
        sink.write(self.url, Ext::Markdown, &self.render()?)
    }

    fn render(&self) -> Result<String> {
        if let Some(selector) = self.selector {
            let input = servo_fetch::extract::ExtractInput::new(&self.page.html, self.url)
                .with_layout_json(self.page.layout_json.as_deref())
                .with_inner_text(Some(&self.page.inner_text))
                .with_selector(Some(selector));
            let text = servo_fetch::extract::extract_text(&input)?;
            if text.is_empty() {
                tracing::warn!(selector, "no elements matched the selector");
            }
            Ok(text)
        } else {
            Ok(self.page.markdown_with_url(self.url)?)
        }
    }
}

pub(crate) struct Json<'a> {
    pub page: &'a Page,
    pub url: &'a str,
    pub selector: Option<&'a str>,
}

impl Json<'_> {
    pub(crate) fn execute(&self, sink: Sink<'_>) -> Result<()> {
        sink.writeln(self.url, Ext::Json, &self.render()?)
    }

    /// Emit a single-line NDJSON record for batch output.
    pub(crate) fn execute_compact(&self, sink: Sink<'_>) -> Result<()> {
        let pretty = self.render()?;
        let line = serde_json::from_str::<serde_json::Value>(&pretty)
            .ok()
            .and_then(|v| serde_json::to_string(&v).ok())
            .unwrap_or(pretty);
        sink.writeln(self.url, Ext::Json, &line)
    }

    fn render(&self) -> Result<String> {
        if let Some(selector) = self.selector {
            let input = servo_fetch::extract::ExtractInput::new(&self.page.html, self.url)
                .with_layout_json(self.page.layout_json.as_deref())
                .with_inner_text(Some(&self.page.inner_text))
                .with_selector(Some(selector));
            Ok(servo_fetch::extract::extract_json(&input)?)
        } else {
            Ok(self.page.extract_json_with_url(self.url)?)
        }
    }
}

pub(crate) struct Screenshot<'a> {
    pub page: &'a Page,
    pub path: &'a Path,
}

impl Screenshot<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        match self.page.screenshot_png() {
            Some(png) => {
                fs::write(self.path, png)?;
                tracing::info!(path = %self.path.display(), "screenshot saved");
                Ok(())
            }
            None => bail!("failed to capture screenshot — the page may not have rendered correctly"),
        }
    }
}

pub(crate) fn js_eval(url: &str, result: &str, sink: Sink<'_>) -> Result<()> {
    sink.writeln(url, Ext::Text, result)
}

pub(crate) struct Extracted<'a> {
    pub page: &'a Page,
    pub url: &'a str,
}

impl Extracted<'_> {
    pub(crate) fn execute(&self, sink: Sink<'_>) -> Result<()> {
        sink.writeln(self.url, Ext::Json, &serde_json::to_string_pretty(&self.payload())?)
    }

    /// Emit a single-line NDJSON record for batch output.
    pub(crate) fn execute_compact(&self, sink: Sink<'_>) -> Result<()> {
        sink.writeln(self.url, Ext::Json, &serde_json::to_string(&self.payload())?)
    }

    fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "url": self.url,
            "extracted": self.page.extracted.as_ref().unwrap_or(&serde_json::Value::Null),
        })
    }
}

pub(crate) fn raw(url: &str, ext: Ext, content: &str, sink: Sink<'_>) -> Result<()> {
    sink.write(url, ext, content)
}

/// Build a filesystem-safe filename from a URL with a stable hash suffix
/// to keep distinct URLs in distinct files.
fn slug_from_url(url: &str, ext: Ext) -> String {
    const MAX_STEM: usize = 180;

    let stripped = url::Url::parse(url).ok().map_or_else(
        || url.to_owned(),
        |u| {
            use std::fmt::Write as _;
            let mut s = u.host_str().unwrap_or("").to_owned();
            if let Some(p) = u.port() {
                let _ = write!(s, ":{p}");
            }
            s.push_str(u.path());
            if let Some(q) = u.query() {
                s.push('?');
                s.push_str(q);
            }
            s
        },
    );

    let mut stem = String::with_capacity(stripped.len());
    let mut prev_us = true;
    for c in stripped.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' => {
                stem.push(c);
                prev_us = false;
            }
            _ if !prev_us => {
                stem.push('_');
                prev_us = true;
            }
            _ => {}
        }
    }
    let stem = stem.trim_matches(['_', '.']);
    let stem = if stem.is_empty() { "index" } else { stem };

    let end = servo_fetch::sanitize::floor_char_boundary(stem, MAX_STEM);
    let stem = &stem[..end];

    format!("{stem}-{:016x}.{ext}", fnv1a64(&stripped))
}

/// FNV-1a 64-bit. Stable across runs and platforms; collision probability is
/// ~3e-10 at 100k URLs (vs ~1.2% for the 32-bit variant at 10k URLs).
fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::*;

    fn ext(s: &str) -> Option<&OsStr> {
        Path::new(s).extension()
    }

    #[test]
    fn slug_strips_scheme_and_replaces_unsafe_chars() {
        let s = slug_from_url("https://example.com/foo/bar?x=1", Ext::Markdown);
        assert!(s.starts_with("example.com_foo_bar_x_1-"));
        assert_eq!(ext(&s), Some(OsStr::new("md")));
    }

    #[test]
    fn slug_collapses_runs_and_trims_underscores() {
        let s = slug_from_url("https://example.com//foo///bar//", Ext::Json);
        assert!(s.starts_with("example.com_foo_bar-"));
        assert_eq!(ext(&s), Some(OsStr::new("json")));
    }

    #[test]
    fn slug_distinct_for_distinct_urls() {
        let a = slug_from_url("https://a.test/x", Ext::Markdown);
        let b = slug_from_url("https://a.test/y", Ext::Markdown);
        assert_ne!(a, b);
    }

    #[test]
    fn slug_stable_across_calls() {
        let a = slug_from_url("https://a.test/x", Ext::Markdown);
        let b = slug_from_url("https://a.test/x", Ext::Markdown);
        assert_eq!(a, b);
    }

    #[test]
    fn slug_handles_empty_path() {
        let s = slug_from_url("https://example.com", Ext::Markdown);
        assert!(s.starts_with("example.com-"));
    }

    #[test]
    fn slug_truncates_long_urls() {
        let url = format!("https://example.com/{}", "a".repeat(500));
        let s = slug_from_url(&url, Ext::Markdown);
        assert!(s.len() < 220, "len was {}", s.len());
        assert_eq!(ext(&s), Some(OsStr::new("md")));
    }

    #[test]
    fn slug_handles_unicode() {
        let s = slug_from_url("https://example.com/日本語", Ext::Markdown);
        assert!(s.contains("example.com"));
        assert_eq!(ext(&s), Some(OsStr::new("md")));
    }

    #[test]
    fn slug_handles_invalid_url() {
        let s = slug_from_url("not a url", Ext::Markdown);
        assert!(s.contains("not_a_url"));
        assert_eq!(ext(&s), Some(OsStr::new("md")));
    }

    #[test]
    fn slug_strips_credentials_and_fragment() {
        let s = slug_from_url("https://user:secret@example.com/foo#anchor", Ext::Markdown);
        assert!(!s.contains("user"), "must not leak username, got: {s}");
        assert!(!s.contains("secret"), "must not leak password, got: {s}");
        assert!(!s.contains("anchor"), "must drop fragment, got: {s}");
        assert!(s.starts_with("example.com_foo-"));
    }

    #[test]
    fn slug_credentials_do_not_affect_filename() {
        let with_creds = slug_from_url("https://user:secret@example.com/foo", Ext::Markdown);
        let without_creds = slug_from_url("https://example.com/foo", Ext::Markdown);
        assert_eq!(with_creds, without_creds, "credentials must not change filename");
    }

    #[test]
    fn slug_includes_non_default_port() {
        let s = slug_from_url("https://example.com:8080/foo", Ext::Markdown);
        assert!(s.contains("8080"), "must include non-default port, got: {s}");
    }
}
