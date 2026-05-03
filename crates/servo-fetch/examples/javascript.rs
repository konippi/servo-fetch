//! Execute JavaScript in a page.

use servo_fetch::{FetchOptions, fetch};

fn main() -> Result<(), servo_fetch::Error> {
    let page = fetch(FetchOptions::javascript("https://example.com", "document.title"))?;

    println!("JS result: {:?}", page.js_result);

    for msg in &page.console_messages {
        println!("[{:?}] {}", msg.level, msg.message);
    }

    Ok(())
}
