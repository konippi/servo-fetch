//! Extract structured JSON from a page using a CSS-selector schema.

use servo_fetch::schema::ExtractSchema;
use servo_fetch::{FetchOptions, fetch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let product_schema = ExtractSchema::from_json(
        r#"{
        "base_selector": ".product",
        "fields": [
            { "name": "title", "selector": "h2", "type": "text" },
            { "name": "price", "selector": ".price", "type": "text" },
            { "name": "url", "selector": "a", "type": "attribute", "attribute": "href" }
        ]
    }"#,
    )?;

    let page = fetch(FetchOptions::new("https://shop.example.com").schema(product_schema))?;

    match page.extracted {
        Some(value) => println!("{}", serde_json::to_string_pretty(&value)?),
        None => println!("no structured data extracted"),
    }

    Ok(())
}
