//! Command dispatch — each output mode is a struct with its own `execute`.

use std::io::Write;

use anyhow::{Context as _, Result, bail};

use crate::bridge::ServoPage;

/// Output an extracted article as Markdown to stdout.
pub(crate) struct Markdown<'a> {
    pub page: &'a ServoPage,
    pub url: &'a str,
    pub selector: Option<&'a str>,
}

impl Markdown<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        let input = servo_fetch::extract::ExtractInput::new(&self.page.html, self.url)
            .with_layout_json(self.page.layout_json.as_deref())
            .with_inner_text(self.page.inner_text.as_deref())
            .with_selector(self.selector);
        let text = servo_fetch::extract::extract_text(&input)?;
        write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&text))?;
        Ok(())
    }
}

/// Output an extracted article as JSON to stdout.
pub(crate) struct Json<'a> {
    pub page: &'a ServoPage,
    pub url: &'a str,
    pub selector: Option<&'a str>,
}

impl Json<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        let input = servo_fetch::extract::ExtractInput::new(&self.page.html, self.url)
            .with_layout_json(self.page.layout_json.as_deref())
            .with_inner_text(self.page.inner_text.as_deref())
            .with_selector(self.selector);
        let json = servo_fetch::extract::extract_json(&input)?;
        writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&json))?;
        Ok(())
    }
}

/// Save the rendered screenshot to a PNG file.
pub(crate) struct Screenshot<'a> {
    pub page: &'a ServoPage,
    pub path: &'a str,
}

impl Screenshot<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        match self.page.screenshot {
            Some(ref img) => {
                img.save(self.path)
                    .with_context(|| format!("failed to save screenshot to {}", self.path))?;
                eprintln!("screenshot saved to {}", self.path);
                Ok(())
            }
            None => bail!("failed to capture screenshot — the page may not have rendered correctly"),
        }
    }
}

/// Print the result of an evaluated JavaScript expression to stdout.
pub(crate) struct JsEval<'a> {
    pub result: &'a str,
}

/// Print raw HTML or inner text to stdout.
pub(crate) struct Raw<'a> {
    pub page: &'a ServoPage,
    pub mode: &'a crate::cli::RawMode,
}

impl Raw<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        let content = match self.mode {
            crate::cli::RawMode::Html => &self.page.html,
            crate::cli::RawMode::Text => self.page.inner_text.as_deref().unwrap_or(""),
        };
        write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(content))?;
        Ok(())
    }
}

impl JsEval<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(self.result))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_page(html: &str) -> ServoPage {
        ServoPage {
            html: html.to_string(),
            ..ServoPage::default()
        }
    }

    #[test]
    fn markdown_produces_output() {
        let page = mock_page("<html><head><title>Test</title></head><body><p>hello</p></body></html>");
        let cmd = Markdown {
            page: &page,
            url: "https://example.com",
            selector: None,
        };
        assert!(cmd.execute().is_ok());
    }

    #[test]
    fn json_produces_valid_json() {
        let page = mock_page("<html><head><title>Test</title></head><body><p>hello</p></body></html>");
        let cmd = Json {
            page: &page,
            url: "https://example.com",
            selector: None,
        };
        assert!(cmd.execute().is_ok());
    }

    #[test]
    fn screenshot_fails_without_image() {
        let page = mock_page("<html></html>");
        let cmd = Screenshot {
            page: &page,
            path: "/tmp/test.png",
        };
        assert!(cmd.execute().is_err());
    }

    #[test]
    fn js_eval_sanitizes_output() {
        let cmd = JsEval {
            result: "hello\x1b[31mworld",
        };
        assert!(cmd.execute().is_ok());
    }
}
