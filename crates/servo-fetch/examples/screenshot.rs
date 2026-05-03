//! Capture a full-page screenshot.

use servo_fetch::{FetchOptions, fetch};

fn main() -> Result<(), servo_fetch::Error> {
    let page = fetch(FetchOptions::screenshot("https://example.com", true))?;

    match page.screenshot_png() {
        Some(png) => {
            std::fs::write("example.png", png)?;
            println!("Saved screenshot to example.png ({} bytes)", png.len());
        }
        None => eprintln!("No screenshot captured"),
    }

    Ok(())
}
