//! Crawl a site with streaming results.

use servo_fetch::{CrawlOptions, crawl_each};

fn main() -> Result<(), servo_fetch::Error> {
    crawl_each(
        CrawlOptions::new("https://example.com").limit(5).max_depth(1),
        |result| match &result.outcome {
            Ok(page) => {
                println!("✓ {} (depth={}, links={})", result.url, result.depth, page.links_found);
            }
            Err(e) => {
                println!("✗ {} — {e}", result.url);
            }
        },
    )?;

    Ok(())
}
