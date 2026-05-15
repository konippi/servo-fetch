"""Live fetch — marked as integration; requires network + Servo engine."""

from __future__ import annotations

import os

import pytest

import servo_fetch

pytestmark = pytest.mark.skipif(
    os.environ.get("SERVO_FETCH_E2E") != "1",
    reason="set SERVO_FETCH_E2E=1 to run end-to-end tests",
)

URL = os.environ.get("SERVO_FETCH_TEST_URL", "https://example.com")


def test_fetch_returns_page() -> None:
    page = servo_fetch.fetch(URL)
    assert isinstance(page, servo_fetch.Page)
    assert page.url == URL
    assert isinstance(page.html, str)
    assert isinstance(page.inner_text, str)


def test_page_markdown_cached() -> None:
    page = servo_fetch.fetch(URL)
    md1 = page.markdown
    md2 = page.markdown
    assert md1 == md2
    assert isinstance(md1, str)


def test_screenshot_none_when_not_requested() -> None:
    page = servo_fetch.fetch(URL)
    assert page.screenshot is None


def test_js_result_none_when_not_requested() -> None:
    page = servo_fetch.fetch(URL)
    assert page.js_result is None


def test_client_fetch() -> None:
    client = servo_fetch.Client(user_agent="servo-fetch-test/1.0")
    page = client.fetch(URL)
    assert page.url == URL
