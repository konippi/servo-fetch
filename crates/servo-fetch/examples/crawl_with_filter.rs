//! Crawl a site with filters and extraction options.

use std::time::Duration;

use servo_fetch::{CrawlOptions, crawl_each};

fn main() -> Result<(), servo_fetch::Error> {
    let opts = CrawlOptions::new("https://example.com")
        .limit(20)
        .max_depth(2)
        .timeout(Duration::from_secs(45))
        .settle(Duration::from_secs(1))
        .include(&["/docs/**", "/blog/**"])
        .exclude(&["/docs/archive/**", "/blog/tags/**"])
        .selector("main")
        .json(false)
        .user_agent("servo-fetch crawl_with_filter example");

    crawl_each(opts, |result| match &result.outcome {
        Ok(page) => {
            println!("✓ {} (depth={}, title={:?})", result.url, result.depth, page.title);
        }
        Err(e) => {
            println!("✗ {} — {e}", result.url);
        }
    })?;

    Ok(())
}
