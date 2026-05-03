//! Extract structured JSON from a page.

fn main() -> Result<(), servo_fetch::Error> {
    let json = servo_fetch::extract_json("https://example.com")?;
    println!("{json}");
    Ok(())
}
