//! Fetch a page and extract Markdown.

fn main() -> Result<(), servo_fetch::Error> {
    let md = servo_fetch::markdown("https://example.com")?;
    println!("{md}");
    Ok(())
}
