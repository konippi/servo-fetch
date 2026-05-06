"""Unit tests for measure.py regex parsers + human()."""
from __future__ import annotations

import re
from pathlib import Path

import pytest

from servo_fetch_bench.measure import _GNU_RSS, _MAC_PEAK, _MAC_RSS, human, peak_bytes

_FIXTURES = Path(__file__).parent / "fixtures"


def test_mac_peak_regex_extracts_peak_memory_footprint() -> None:
    text = (_FIXTURES / "time-l-macos.txt").read_text()
    m = _MAC_PEAK.search(text)
    assert m is not None
    assert int(m.group(1)) == 7_848_296


def test_mac_rss_regex_falls_back_when_peak_line_absent() -> None:
    text = (_FIXTURES / "time-l-macos.txt").read_text()
    # Strip the peak line to force the fallback branch.
    stripped = re.sub(r"^.*peak memory footprint$", "", text, flags=re.MULTILINE)
    assert _MAC_PEAK.search(stripped) is None
    m = _MAC_RSS.search(stripped)
    assert m is not None
    assert int(m.group(1)) == 15_810_560


def test_gnu_rss_regex_extracts_maximum_resident_set_size() -> None:
    text = (_FIXTURES / "time-v-gnu.txt").read_text()
    m = _GNU_RSS.search(text)
    assert m is not None
    assert int(m.group(1)) == 15_432


@pytest.mark.parametrize(
    "pattern", [_MAC_PEAK, _MAC_RSS, _GNU_RSS],
    ids=["mac-peak", "mac-rss", "gnu-rss"],
)
def test_regex_returns_none_on_empty_input(pattern: re.Pattern[str]) -> None:
    assert pattern.search("") is None


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (0, "0 B"),
        (512, "512 B"),
        (1023, "1023 B"),
        (1024, "1.0 KB"),
        (1_048_575, "1024.0 KB"),
        (1_048_576, "1.0 MB"),
        (15_810_560, "15.1 MB"),
        (1_073_741_823, "1024.0 MB"),
        (1_073_741_824, "1.00 GB"),
        (1_610_612_736, "1.50 GB"),
    ],
)
def test_human_renders_bytes_in_appropriate_unit(value: int, expected: str) -> None:
    assert human(value) == expected


# Tree-polling must include descendants; /usr/bin/time -l misses on macOS
# when Chromium helpers detach. The child process below allocates ~30 MiB
# to make the descendant clearly visible above the parent's baseline.
_CHILD_PAYLOAD = (
    "import os, subprocess, sys, time\n"
    "if os.environ.get('CHILD') == '1':\n"
    "    buf = bytearray(30 * 1024 * 1024)\n"
    "    sys.stdout.buffer.write(buf[:10])\n"
    "    time.sleep(0.3)\n"
    "else:\n"
    "    env = dict(os.environ, CHILD='1')\n"
    "    subprocess.run([sys.executable, '-c', sys.argv[1]], env=env, check=True)\n"
)


def test_tree_mode_captures_descendants() -> None:
    peak = peak_bytes(["python3", "-c", _CHILD_PAYLOAD, _CHILD_PAYLOAD], tree=True)
    # Parent alone is a tiny Python interpreter; ≥20 MiB proves the ~30 MiB
    # child was seen too.
    assert peak >= 20 * 1024 * 1024, f"tree peak too small: {peak} bytes"


def test_tree_mode_returns_zero_on_missing_binary() -> None:
    assert peak_bytes(["/nonexistent/xyz"], tree=True) == 0
