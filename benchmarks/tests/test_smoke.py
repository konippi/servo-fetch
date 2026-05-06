"""End-to-end smoke: FixtureServer + scoring pipeline."""
from __future__ import annotations

import http.client
import platform
import time
import urllib.request
from contextlib import closing

import pytest

from servo_fetch_bench.config import Config
from servo_fetch_bench.fixtures import load_local_fixtures, serve
from servo_fetch_bench.measure import peak_bytes
from servo_fetch_bench.scoring import parse_expect, snippet_coverage


@pytest.fixture
def served_fixtures(cfg: Config, free_port: int):
    pages = load_local_fixtures(cfg.fixtures_dir)
    with serve(pages, free_port):
        yield cfg, free_port, pages


def test_static_fixture_roundtrip_matches_expectations(served_fixtures) -> None:
    cfg, port, _ = served_fixtures
    withs, withouts = parse_expect(cfg.fixtures_dir / "perf" / "golden" / "static-small.expect")
    with urllib.request.urlopen(
        f"http://127.0.0.1:{port}/perf/static-small.html", timeout=5,
    ) as resp:
        html = resp.read().decode("utf-8")
    coverage = snippet_coverage(html, withs, withouts)
    assert coverage.with_coverage == 1.0, coverage.missing


def test_fixture_server_serves_bytes_verbatim(served_fixtures) -> None:
    _, port, pages = served_fixtures
    for path_key, original in list(pages.items())[:3]:
        with urllib.request.urlopen(
            f"http://127.0.0.1:{port}{path_key}", timeout=5,
        ) as resp:
            served = resp.read()
        assert served == original, path_key


def test_fixture_server_health_endpoint_returns_ok(served_fixtures) -> None:
    _, port, _ = served_fixtures
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=5) as resp:
        body = resp.read()
    assert body == b"ok\n"


def test_fixture_server_returns_404_for_missing_path(served_fixtures) -> None:
    _, port, _ = served_fixtures
    with closing(http.client.HTTPConnection("127.0.0.1", port, timeout=5)) as conn:
        conn.request("GET", "/missing.html")
        assert conn.getresponse().status == 404


def test_peak_bytes_measurement_is_bounded_for_short_process() -> None:
    # `sleep 0.1` guarantees ≥10 psutil poll windows (10 ms cadence); a
    # bare `echo` sometimes exits before the first sample and returns 0.
    start = time.perf_counter()
    peak = peak_bytes(["sleep", "0.1"])
    elapsed = time.perf_counter() - start
    assert elapsed < 3.0
    assert peak < 50 * 1024 * 1024, f"unreasonable: {peak}"
    if platform.system() == "Darwin":
        assert peak > 0
