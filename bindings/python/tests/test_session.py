"""Isolated browser sessions — requires Servo engine."""

from __future__ import annotations

import asyncio
import os

import pytest

import servo_fetch

pytestmark = pytest.mark.skipif(
    os.environ.get("SERVO_FETCH_E2E") != "1",
    reason="set SERVO_FETCH_E2E=1 to run end-to-end tests",
)

URL = os.environ.get("SERVO_FETCH_TEST_URL", "https://example.com")


def test_session_fetch_and_close() -> None:
    with servo_fetch.Session() as session:
        page = session.fetch(URL)
        assert isinstance(page, servo_fetch.Page)
        assert page.url == URL


def test_session_close_is_terminal_and_idempotent() -> None:
    session = servo_fetch.Session()
    assert not session.is_closed
    session.close()
    session.close()
    assert session.is_closed
    with pytest.raises(servo_fetch.ServoFetchError, match="browser session is closed"):
        session.fetch(URL)


def test_async_session_fetch_and_close() -> None:
    async def scenario() -> None:
        async with servo_fetch.AsyncSession() as session:
            page = await session.fetch(URL)
            assert isinstance(page, servo_fetch.Page)

    asyncio.run(scenario())
