"""Fetch multiple URLs with shared client configuration."""

from servo_fetch import Client

urls = [
    "https://example.com",
    "https://example.org",
]

client = Client(timeout=30, user_agent="batch-bot/1.0")

for url in urls:
    page = client.fetch(url)
    print(f"{url}: {page.title} ({len(page.html)} bytes)")
