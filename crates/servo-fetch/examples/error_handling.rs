//! Handle different error types.

use servo_fetch::{Error, FetchOptions, fetch};

fn main() {
    let urls = [
        "https://example.com",
        "not a url",
        "http://127.0.0.1/",
        "file:///etc/passwd",
    ];

    for url in urls {
        match fetch(FetchOptions::new(url)) {
            Ok(page) => println!("✓ {url} — {} bytes", page.html.len()),
            Err(e) if e.is_timeout() => println!("⏱ {url} — timeout, retrying..."),
            Err(Error::InvalidUrl { reason, .. }) => println!("✗ {url} — {reason}"),
            Err(e) => println!("✗ {url} — {e}"),
        }
    }
}
