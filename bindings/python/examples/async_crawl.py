"""Crawl a site asynchronously, streaming results as they arrive."""

import asyncio

from servo_fetch import AsyncClient


async def main():
    async with AsyncClient(user_agent="example-bot/1.0") as client:
        async for result in client.crawl_stream(
            "https://example.com",
            max_pages=5,
            max_depth=1,
        ):
            print(f"[depth={result.depth}] {result.url} — {result.title}")


asyncio.run(main())
