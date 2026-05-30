//! Fetch a page and extract Markdown.

#[tokio::main]
async fn main() -> Result<(), servo_fetch::Error> {
    let md = servo_fetch::markdown("https://example.com").await?;
    println!("{md}");
    Ok(())
}
