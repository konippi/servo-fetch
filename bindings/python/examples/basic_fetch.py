"""Basic fetch: render a page and access all representations."""

import servo_fetch

page = servo_fetch.fetch("https://example.com")
print(f"Title: {page.title}")
print(f"HTML length: {len(page.html)}")
print(f"Text: {page.inner_text[:100]}...")
print(f"\nMarkdown:\n{page.markdown[:200]}...")
