"""Extract structured data from a page using a CSS-selector schema."""

import json

from servo_fetch import Field, Schema, fetch

schema = Schema(
    base_selector="article.product_pod",
    fields=[
        Field(name="title", selector="h3 a", type="attribute", attribute="title"),
        Field(name="price", selector=".price_color", type="text"),
        Field(name="url", selector="h3 a", type="attribute", attribute="href"),
    ],
)

page = fetch("https://books.toscrape.com", schema=schema)
print(json.dumps(page.extracted[:3], indent=2))  # first 3 products
