//! Domain-to-wire conversions.

use serde_json::Value;
use servo_fetch::CrawlResult;
use servo_fetch::extract::ArticleData;
use servo_fetch_types as wire;

pub(crate) fn article(a: ArticleData) -> wire::Article {
    let ArticleData {
        title,
        content,
        text_content,
        byline,
        excerpt,
        lang,
        url,
    } = a;
    wire::Article {
        title,
        content,
        text_content,
        byline,
        excerpt,
        lang,
        url,
    }
}

pub(crate) fn mapped_url(m: &servo_fetch::MappedUrl) -> wire::MappedUrl {
    let servo_fetch::MappedUrl { url, lastmod } = m;
    wire::MappedUrl {
        url: url.clone(),
        lastmod: lastmod.clone(),
    }
}

pub(crate) fn crawl_event(result: CrawlResult) -> wire::CrawlEvent {
    let fetched_at = humantime::format_rfc3339_millis(result.fetched_at).to_string();
    let depth = result.depth as u64;
    match result.outcome {
        Ok(page) => wire::CrawlEvent::Page {
            url: result.url,
            depth,
            fetched_at,
            title: page.title,
            content: page.content,
            links_found: page.links_found as u64,
        },
        Err(e) => wire::CrawlEvent::Error {
            url: result.url,
            depth,
            fetched_at,
            error: e.to_string(),
        },
    }
}

pub(crate) fn crawl_stats(crawled: u64, errors: u64, elapsed_ms: u64) -> wire::CrawlEvent {
    wire::CrawlEvent::Stats {
        crawled,
        errors,
        elapsed_ms,
    }
}

pub(crate) fn schema_extract(url: &str, extracted: Value) -> wire::SchemaExtractResult {
    wire::SchemaExtractResult {
        url: url.to_owned(),
        extracted,
    }
}

fn console_level(level: servo_fetch::ConsoleLevel) -> wire::ConsoleLevel {
    use servo_fetch::ConsoleLevel as C;
    match level {
        C::Info => wire::ConsoleLevel::Info,
        C::Warn => wire::ConsoleLevel::Warn,
        C::Error => wire::ConsoleLevel::Error,
        C::Debug => wire::ConsoleLevel::Debug,
        C::Trace => wire::ConsoleLevel::Trace,
        _ => wire::ConsoleLevel::Log,
    }
}

pub(crate) fn evaluate_result(
    url: String,
    result: String,
    console: &[servo_fetch::ConsoleMessage],
) -> wire::EvaluateResult {
    wire::EvaluateResult {
        url,
        result,
        console: console
            .iter()
            .map(|m| wire::ConsoleMessage {
                level: console_level(m.level),
                message: m.message.clone(),
            })
            .collect(),
    }
}
