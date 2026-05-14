"""Input validation — offline, no network required."""

from __future__ import annotations

import pytest

import servo_fetch


@pytest.mark.parametrize(
    ("kwargs", "match"),
    [
        ({"timeout": -1.0}, "timeout"),
        ({"timeout": 0.0}, "timeout"),
        ({"timeout": float("inf")}, "timeout"),
        ({"timeout": float("nan")}, "timeout"),
        ({"timeout": 3601.0}, "timeout"),
        ({"settle": -1.0}, "settle"),
        ({"settle": 61.0}, "settle"),
        ({"javascript": "a" * 1_000_001}, "javascript source length"),
    ],
    ids=[
        "timeout_negative",
        "timeout_zero",
        "timeout_inf",
        "timeout_nan",
        "timeout_above_max",
        "settle_negative",
        "settle_above_max",
        "js_too_long",
    ],
)
def test_fetch_rejects_invalid_kwargs(kwargs: dict, match: str) -> None:
    with pytest.raises(ValueError, match=match):
        servo_fetch.fetch("https://example.com", **kwargs)


def test_url_length_capped() -> None:
    with pytest.raises(ValueError, match="URL length"):
        servo_fetch.fetch("https://example.com/" + "a" * 9000)


@pytest.mark.parametrize(
    ("kwargs", "match"),
    [
        ({"timeout": -1.0}, "timeout"),
        ({"timeout": 0.0}, "timeout"),
        ({"settle": -0.1}, "settle"),
    ],
    ids=["client_timeout_neg", "client_timeout_zero", "client_settle_neg"],
)
def test_client_rejects_invalid_kwargs(kwargs: dict, match: str) -> None:
    with pytest.raises(ValueError, match=match):
        servo_fetch.Client(**kwargs)


def test_timeout_at_max_accepted() -> None:
    """3600.0 is the ceiling — should not raise."""
    with pytest.raises(Exception, match=r"(?!timeout must)"):
        servo_fetch.fetch("https://192.0.2.1", timeout=3600.0)
