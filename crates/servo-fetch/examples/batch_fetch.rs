//! Fetch multiple URLs concurrently using `JoinSet`.

use servo_fetch::{FetchOptions, fetch};
use tokio::task::JoinSet;

#[tokio::main]
async fn main() {
    let urls = ["https://example.com", "https://www.rust-lang.org", "https://servo.org"];

    let mut set = JoinSet::new();
    for url in urls {
        let opts = FetchOptions::new(url);
        set.spawn(async move { (url, fetch(&opts).await) });
    }

    while let Some(result) = set.join_next().await {
        match result.unwrap() {
            (url, Ok(page)) => {
                let md = page.markdown().unwrap_or_default();
                let preview: String = md.chars().take(200).collect();
                println!("✓ {url} — {} bytes\n{preview}\n", page.html.len());
            }
            (url, Err(e)) => println!("✗ {url} — {e}\n"),
        }
    }
}
