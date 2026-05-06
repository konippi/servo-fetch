"""Unit tests for size measurement (no real binaries)."""
from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest

from servo_fetch_bench import sizes
from servo_fetch_bench.config import Config


@pytest.fixture
def all_bins_missing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Override every tracked binary path to a non-existent location."""
    for attr, name in (
        ("servo_fetch_bin", "nope-sf"),
        ("chrome_headless_shell_bin", "nope-chs"),
        ("lightpanda_bin", "nope-lp"),
    ):
        monkeypatch.setattr(Config, attr, tmp_path / name)


@pytest.mark.parametrize(
    ("line", "expected"),
    [
        ("/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)", False),
        ("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", False),
        ("/lib/x86_64-linux-gnu/libc.so.6 (0x...)", False),
        ("/lib64/ld-linux.so.2 (0x...)", False),
        ("/opt/homebrew/lib/libglib.dylib", True),
        ("/usr/local/lib/libssl.so.1.1", True),
        ("target/release/servo-fetch:", False),  # otool header line
        ("linux-vdso.so.1 (0x...)", False),       # ldd kernel-provided vdso
        ("", False),
        ("  (0x...)", False),
    ],
)
def test_is_non_system_dep_classifies_lib_lines(line: str, expected: bool) -> None:
    assert sizes._is_non_system_dep(line) is expected


def test_binary_targets_report_none_for_missing_binaries(
    cfg: Config, all_bins_missing: None,
) -> None:
    targets = sizes.binary_targets(cfg)
    labels = [t[0] for t in targets]
    paths = [t[1] for t in targets]
    assert labels[:3] == ["servo-fetch", "chrome-headless-shell", "lightpanda"]
    assert all(p is None for p in paths[:3])


def test_binary_targets_resolve_existing_binary(
    cfg: Config,
    make_stub_bin: Callable[..., Path],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stub = make_stub_bin("servo-fetch", content=b"\x7fELF placeholder")
    monkeypatch.setattr(Config, "servo_fetch_bin", stub)
    targets = {t[0]: t[1] for t in sizes.binary_targets(cfg)}
    assert targets["servo-fetch"] == stub


def test_measure_returns_entry_for_every_required_label(
    cfg: Config, all_bins_missing: None,
) -> None:
    labels = [e.label for e in sizes.measure(cfg)]
    assert {"servo-fetch", "chrome-headless-shell", "lightpanda"} <= set(labels)


def test_measure_missing_binary_produces_none_metrics(
    cfg: Config, tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(Config, "servo_fetch_bin", tmp_path / "nope")
    sf = next(e for e in sizes.measure(cfg) if e.label == "servo-fetch")
    assert sf.binary is None
    assert sf.size_bytes is None
    assert sf.size_mib is None
    assert sf.non_system_deps is None


def test_measure_present_binary_reports_size_in_mib(
    cfg: Config, tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    stub = tmp_path / "servo-fetch"
    stub.write_bytes(b"x" * (1024 * 1024 * 2 + 500_000))  # ~2.5 MiB
    monkeypatch.setattr(Config, "servo_fetch_bin", stub)
    # Isolate size computation from otool/ldd; tests must not require them.
    monkeypatch.setattr(sizes, "non_system_deps", lambda _p: 0)
    sf = next(e for e in sizes.measure(cfg) if e.label == "servo-fetch")
    assert sf.size_bytes == stub.stat().st_size
    assert sf.size_mib is not None
    assert 2.3 < sf.size_mib < 2.7


def test_size_entry_size_mib_is_none_when_size_bytes_is_none() -> None:
    assert sizes.SizeEntry("x", None, None, None).size_mib is None


def test_size_entry_size_mib_uses_base2_units() -> None:
    e = sizes.SizeEntry("x", Path("/tmp/x"), 10 * 1024 * 1024, None)
    assert e.size_mib == 10.0
