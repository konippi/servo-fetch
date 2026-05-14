"""Async API: thin `asyncio.to_thread` wrappers over the sync Rust core."""

from __future__ import annotations

import asyncio
import threading
from collections.abc import AsyncIterator
from typing import cast

from ._native import Client as _SyncClient
from ._native import CrawlResult, MappedUrl, Page, Schema
from ._native import fetch as _sync_fetch

__all__ = ["AsyncClient", "fetch_async"]

_SENTINEL = object()


async def fetch_async(
    url: str,
    *,
    timeout: float | None = None,
    settle: float | None = None,
    user_agent: str | None = None,
    screenshot: bool = False,
    javascript: str | None = None,
    schema: Schema | None = None,
) -> Page:
    """Asynchronously fetch a single URL."""
    return await asyncio.to_thread(
        _sync_fetch,
        url,
        timeout=timeout,
        settle=settle,
        user_agent=user_agent,
        screenshot=screenshot,
        javascript=javascript,
        schema=schema,
    )


class AsyncClient:
    """Async client with shared configuration."""

    def __init__(
        self,
        *,
        timeout: float = 30.0,
        settle: float = 0.0,
        user_agent: str | None = None,
    ) -> None:
        self._inner = _SyncClient(timeout=timeout, settle=settle, user_agent=user_agent)

    async def fetch(
        self,
        url: str,
        *,
        timeout: float | None = None,
        settle: float | None = None,
        screenshot: bool = False,
        javascript: str | None = None,
        schema: Schema | None = None,
    ) -> Page:
        return await asyncio.to_thread(
            self._inner.fetch,
            url,
            timeout=timeout,
            settle=settle,
            screenshot=screenshot,
            javascript=javascript,
            schema=schema,
        )

    async def crawl(
        self,
        url: str,
        *,
        max_pages: int = 50,
        max_depth: int = 3,
        include: str | list[str] | None = None,
        exclude: str | list[str] | None = None,
        concurrency: int = 1,
        delay_ms: int = 0,
    ) -> list[CrawlResult]:
        return await asyncio.to_thread(
            self._inner.crawl,
            url,
            max_pages=max_pages,
            max_depth=max_depth,
            include=include,
            exclude=exclude,
            concurrency=concurrency,
            delay_ms=delay_ms,
        )

    async def crawl_stream(
        self,
        url: str,
        *,
        max_pages: int = 50,
        max_depth: int = 3,
        include: str | list[str] | None = None,
        exclude: str | list[str] | None = None,
        concurrency: int = 1,
        delay_ms: int = 0,
    ) -> AsyncIterator[CrawlResult]:
        """Crawl a site, yielding each page as it completes."""
        loop = asyncio.get_running_loop()
        queue: asyncio.Queue[CrawlResult | object] = asyncio.Queue(maxsize=16)
        abort = threading.Event()

        def on_page(result: CrawlResult) -> None:
            fut = asyncio.run_coroutine_threadsafe(queue.put(result), loop)
            fut.result()

        def producer() -> None:
            try:
                self._inner.crawl_each(
                    url,
                    on_page,
                    abort=abort,
                    max_pages=max_pages,
                    max_depth=max_depth,
                    include=include,
                    exclude=exclude,
                    concurrency=concurrency,
                    delay_ms=delay_ms,
                )
            finally:
                fut = asyncio.run_coroutine_threadsafe(queue.put(_SENTINEL), loop)
                fut.result()

        threading.Thread(target=producer, daemon=True).start()
        try:
            while True:
                item = await queue.get()
                if item is _SENTINEL:
                    return
                yield cast(CrawlResult, item)
        finally:
            abort.set()

    async def map(
        self,
        url: str,
        *,
        limit: int = 5000,
        include: str | list[str] | None = None,
        exclude: str | list[str] | None = None,
    ) -> list[MappedUrl]:
        return await asyncio.to_thread(
            self._inner.map,
            url,
            limit=limit,
            include=include,
            exclude=exclude,
        )

    async def __aenter__(self) -> AsyncClient:
        return self

    async def __aexit__(self, *_: object) -> None:
        """No-op; the engine is process-global. Provided for forward compatibility."""
