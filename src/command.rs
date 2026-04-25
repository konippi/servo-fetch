//! Command dispatch — each output mode is a struct with its own `execute`.

use std::io::Write;

use anyhow::{Context as _, Result, bail};

use crate::bridge::ServoPage;

pub struct Markdown<'a> {
    pub page: &'a ServoPage,
    pub url: &'a str,
    pub selector: Option<&'a str>,
}

impl Markdown<'_> {
    pub fn execute(&self) -> Result<()> {
        let mut input = servo_fetch::extract::ExtractInput::new(&self.page.html, self.url);
        input.layout_json = self.page.layout_json.as_deref();
        input.inner_text = self.page.inner_text.as_deref();
        input.selector = self.selector;
        let text = servo_fetch::extract::extract_text(&input)?;
        write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&text))?;
        Ok(())
    }
}

pub struct Json<'a> {
    pub page: &'a ServoPage,
    pub url: &'a str,
    pub selector: Option<&'a str>,
}

impl Json<'_> {
    pub fn execute(&self) -> Result<()> {
        let mut input = servo_fetch::extract::ExtractInput::new(&self.page.html, self.url);
        input.layout_json = self.page.layout_json.as_deref();
        input.inner_text = self.page.inner_text.as_deref();
        input.selector = self.selector;
        let json = servo_fetch::extract::extract_json(&input)?;
        writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&json))?;
        Ok(())
    }
}

pub struct Screenshot<'a> {
    pub page: &'a ServoPage,
    pub path: &'a str,
}

impl Screenshot<'_> {
    pub fn execute(&self) -> Result<()> {
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

pub struct JsEval<'a> {
    pub result: &'a str,
}

pub struct Raw<'a> {
    pub page: &'a ServoPage,
    pub mode: &'a str,
}

impl Raw<'_> {
    pub fn execute(&self) -> Result<()> {
        let content = match self.mode {
            "html" => &self.page.html,
            "text" => self.page.inner_text.as_deref().unwrap_or(""),
            other => bail!("unknown raw mode '{other}'; use 'html' or 'text'"),
        };
        write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(content))?;
        Ok(())
    }
}

impl JsEval<'_> {
    pub fn execute(&self) -> Result<()> {
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
            inner_text: None,
            layout_json: None,
            screenshot: None,
            js_result: None,
            pdf_data: None,
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
