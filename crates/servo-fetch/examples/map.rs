//! Discover URLs from a site's sitemap without rendering pages.

use servo_fetch::{MapOptions, MappedUrl, map};

#[tokio::main]
async fn main() -> Result<(), servo_fetch::Error> {
    let urls: Vec<MappedUrl> = map(
        &MapOptions::new("https://example.com")
            .limit(25)
            .include(&["/**"])
            .exclude(&["/admin/**", "/private/**"])
            .timeout(15),
    )
    .await?;

    for entry in urls {
        match entry.lastmod.as_deref() {
            Some(lastmod) => println!("{} (last modified {lastmod})", entry.url),
            None => println!("{}", entry.url),
        }
    }

    Ok(())
}
