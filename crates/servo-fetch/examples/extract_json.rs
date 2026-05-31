//! Extract structured JSON from a page.

#[tokio::main]
async fn main() -> Result<(), servo_fetch::Error> {
    let json = servo_fetch::extract_json("https://example.com").await?;
    println!("{json}");
    Ok(())
}
