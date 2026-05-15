"""Async API + thread safety — offline tests."""

from __future__ import annotations

import concurrent.futures

import pytest

from servo_fetch import AsyncClient, Schema, fetch_async


@pytest.mark.asyncio
async def test_async_client_context_manager() -> None:
    async with AsyncClient(user_agent="test/1.0") as client:
        assert isinstance(client, AsyncClient)


@pytest.mark.parametrize(
    ("kwargs", "match"),
    [
        ({"timeout": -1.0}, "timeout"),
        ({"settle": -1.0}, "settle"),
    ],
    ids=["timeout_neg", "settle_neg"],
)
def test_async_client_rejects_invalid(kwargs: dict, match: str) -> None:
    with pytest.raises(ValueError, match=match):
        AsyncClient(**kwargs)


@pytest.mark.asyncio
async def test_fetch_async_rejects_bad_timeout() -> None:
    with pytest.raises(ValueError, match="timeout"):
        await fetch_async("https://example.com", timeout=-1.0)


@pytest.mark.asyncio
async def test_fetch_async_rejects_long_url() -> None:
    with pytest.raises(ValueError, match="URL length"):
        await fetch_async("https://example.com/" + "a" * 9000)


def test_schema_extract_thread_safe() -> None:
    """8 threads x 50 calls on a shared frozen Schema."""
    schema = Schema.from_dict({"fields": [{"name": "t", "selector": "h1", "type": "text"}]})
    html = "<html><body><h1>Hello</h1></body></html>"
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as ex:
        results = list(ex.map(lambda _: schema.extract(html), range(50)))
    assert all(r == {"t": "Hello"} for r in results)


def test_schema_base_selector_thread_safe() -> None:
    """Verify base_selector path is also thread-safe."""
    schema = Schema.from_dict({"base_selector": "li", "fields": [{"name": "t", "selector": "", "type": "text"}]})
    html = "<ul><li>a</li><li>b</li></ul>"
    expected = [{"t": "a"}, {"t": "b"}]
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as ex:
        results = list(ex.map(lambda _: schema.extract(html), range(20)))
    assert all(r == expected for r in results)
