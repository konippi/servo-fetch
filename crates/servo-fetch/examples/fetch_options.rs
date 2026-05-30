//! Fetch with custom timeout and settle options.

use std::time::Duration;

use servo_fetch::{FetchOptions, fetch};

#[tokio::main]
async fn main() -> Result<(), servo_fetch::Error> {
    let page = fetch(
        &FetchOptions::new("https://example.com")
            .timeout(Duration::from_secs(60))
            .settle(Duration::from_secs(3)),
    )
    .await?;

    println!("Title: {:?}", page.title);
    println!("HTML length: {}", page.html.len());
    let preview: String = page.inner_text.chars().take(200).collect();
    println!("Text: {preview}");

    let md = page.markdown()?;
    println!("\n--- Markdown ---\n{md}");

    Ok(())
}
