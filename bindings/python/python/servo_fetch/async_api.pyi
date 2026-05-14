"""Type stubs for servo_fetch.async_api."""

from collections.abc import AsyncIterator
from typing import Any

from ._native import CrawlResult, MappedUrl, Page, Schema

__all__ = ["AsyncClient", "fetch_async"]

async def fetch_async(
    url: str,
    *,
    timeout: float | None = None,
    settle: float | None = None,
    user_agent: str | None = None,
    screenshot: bool = False,
    javascript: str | None = None,
    schema: Schema | None = None,
) -> Page: ...

class AsyncClient:
    def __init__(
        self,
        *,
        timeout: float = 30.0,
        settle: float = 0.0,
        user_agent: str | None = None,
    ) -> None: ...
    async def fetch(
        self,
        url: str,
        *,
        timeout: float | None = None,
        settle: float | None = None,
        screenshot: bool = False,
        javascript: str | None = None,
        schema: Schema | None = None,
    ) -> Page: ...
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
    ) -> list[CrawlResult]: ...
    def crawl_stream(
        self,
        url: str,
        *,
        max_pages: int = 50,
        max_depth: int = 3,
        include: str | list[str] | None = None,
        exclude: str | list[str] | None = None,
        concurrency: int = 1,
        delay_ms: int = 0,
    ) -> AsyncIterator[CrawlResult]: ...
    async def map(
        self,
        url: str,
        *,
        limit: int = 5000,
        include: str | list[str] | None = None,
        exclude: str | list[str] | None = None,
    ) -> list[MappedUrl]: ...
    async def __aenter__(self) -> AsyncClient: ...
    async def __aexit__(self, *_: Any) -> None: ...
