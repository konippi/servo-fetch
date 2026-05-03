//! Crawl a site with streaming results.

use servo_fetch::{CrawlOptions, CrawlStatus, crawl_each};

fn main() -> Result<(), servo_fetch::Error> {
    crawl_each(
        CrawlOptions::new("https://example.com").limit(5).max_depth(1),
        |result| {
            let mark = if result.status == CrawlStatus::Ok { "✓" } else { "✗" };
            println!(
                "{mark} {} (depth={}, links={})",
                result.url, result.depth, result.links_found
            );
        },
    )?;

    Ok(())
}
