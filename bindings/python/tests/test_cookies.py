"""Cookie-file loading via the `cookies_file` kwarg — offline error paths."""

from __future__ import annotations

import os
from collections.abc import Callable
from pathlib import Path

import pytest

import servo_fetch
from servo_fetch import CookieError, ServoFetchError, fetch_async

URL = "https://example.com"


def test_cookie_error_subclasses_servo_fetch_error() -> None:
    assert issubclass(CookieError, ServoFetchError)


@pytest.mark.parametrize(
    "call",
    [
        pytest.param(lambda cf: servo_fetch.fetch(URL, cookies_file=cf), id="fetch"),
        pytest.param(lambda cf: servo_fetch.Client().fetch(URL, cookies_file=cf), id="client.fetch"),
        pytest.param(lambda cf: servo_fetch.Client().crawl(URL, cookies_file=cf), id="client.crawl"),
    ],
)
def test_missing_cookie_file_raises(call: Callable[[str], object], tmp_path: Path) -> None:
    with pytest.raises(CookieError, match="failed to load cookies"):
        call(str(tmp_path / "nope.txt"))


def test_cookies_file_wrong_type_raises() -> None:
    with pytest.raises(TypeError, match="PathLike"):
        servo_fetch.fetch(URL, cookies_file=123)


@pytest.mark.asyncio
async def test_fetch_async_missing_cookie_file_raises(tmp_path: Path) -> None:
    with pytest.raises(CookieError, match="failed to load cookies"):
        await fetch_async(URL, cookies_file=str(tmp_path / "nope.txt"))


@pytest.mark.skipif(
    os.environ.get("SERVO_FETCH_E2E") != "1",
    reason="set SERVO_FETCH_E2E=1 to run end-to-end tests",
)
def test_fetch_with_valid_cookie_file(tmp_path: Path) -> None:
    cookies = tmp_path / "cookies.txt"
    cookies.write_text("app.example.com\tFALSE\t/\tFALSE\t0\tsession\tabc123\n")
    url = os.environ.get("SERVO_FETCH_TEST_URL", URL)
    page = servo_fetch.fetch(url, cookies_file=cookies)
    assert isinstance(page, servo_fetch.Page)
