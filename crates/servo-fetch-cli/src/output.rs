//! Output formatters for stdout (Markdown, JSON, screenshot, raw).

use std::io::Write;

use anyhow::{Result, bail};

use servo_fetch::Page;

pub(crate) struct Markdown<'a> {
    pub page: &'a Page,
    pub url: &'a str,
    pub selector: Option<&'a str>,
}

impl Markdown<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        let md = if let Some(selector) = self.selector {
            let input = servo_fetch::extract::ExtractInput::new(&self.page.html, self.url)
                .with_layout_json(self.page.layout_json.as_deref())
                .with_inner_text(Some(&self.page.inner_text))
                .with_selector(Some(selector));
            servo_fetch::extract::extract_text(&input)?
        } else {
            self.page.markdown_with_url(self.url)?
        };
        write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&md))?;
        Ok(())
    }
}

pub(crate) struct Json<'a> {
    pub page: &'a Page,
    pub url: &'a str,
    pub selector: Option<&'a str>,
}

impl Json<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        let json = self.render()?;
        writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&json))?;
        Ok(())
    }

    /// Emit a single-line NDJSON record for batch output.
    pub(crate) fn execute_compact(&self) -> Result<()> {
        let pretty = self.render()?;
        let line = serde_json::from_str::<serde_json::Value>(&pretty)
            .ok()
            .and_then(|v| serde_json::to_string(&v).ok())
            .unwrap_or(pretty);
        writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&line))?;
        Ok(())
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
    pub path: &'a str,
}

impl Screenshot<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        match self.page.screenshot_png() {
            Some(png) => {
                std::fs::write(self.path, png)?;
                tracing::info!(path = %self.path, "screenshot saved");
                Ok(())
            }
            None => bail!("failed to capture screenshot — the page may not have rendered correctly"),
        }
    }
}

pub(crate) struct JsEval<'a> {
    pub result: &'a str,
}

impl JsEval<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(self.result))?;
        Ok(())
    }
}

pub(crate) struct Raw<'a> {
    pub page: &'a Page,
    pub mode: &'a crate::cli::RawMode,
}

impl Raw<'_> {
    pub(crate) fn execute(&self) -> Result<()> {
        let content = match self.mode {
            crate::cli::RawMode::Html => self.page.html.as_str(),
            crate::cli::RawMode::Text => &self.page.inner_text,
        };
        write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(content))?;
        Ok(())
    }
}
