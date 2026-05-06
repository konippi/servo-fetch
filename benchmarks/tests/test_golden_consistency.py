"""Committed golden files match their .expect contracts."""
from __future__ import annotations

from pathlib import Path

import pytest

from servo_fetch_bench.config import Config
from servo_fetch_bench.scoring import parse_expect

_FIXTURES = Config().fixtures_dir
_SPEED_EXPECTS = sorted((_FIXTURES / "perf" / "golden").glob("*.expect"))
_PAGE_TYPE_GOLDEN = _FIXTURES / "extraction" / "golden"
_PAGE_TYPE_TXTS = sorted(_PAGE_TYPE_GOLDEN.glob("*.txt"))


@pytest.mark.parametrize("path", _SPEED_EXPECTS, ids=lambda p: p.stem)
def test_speed_expect_file_has_required_substrings(path: Path) -> None:
    withs, _ = parse_expect(path)
    assert withs, f"{path.name} has no with[] entries"


@pytest.mark.parametrize("txt", _PAGE_TYPE_TXTS, ids=lambda p: p.stem)
def test_page_type_golden_text_contains_all_with_snippets(txt: Path) -> None:
    withs, _ = parse_expect(txt.with_suffix(".expect"))
    content = txt.read_text(encoding="utf-8")
    missing = [w for w in withs if w not in content]
    assert not missing, f"{txt.name} is missing: {missing}"


@pytest.mark.parametrize("txt", _PAGE_TYPE_TXTS, ids=lambda p: p.stem)
def test_page_type_golden_text_excludes_all_without_snippets(txt: Path) -> None:
    _, withouts = parse_expect(txt.with_suffix(".expect"))
    content = txt.read_text(encoding="utf-8")
    leaked = [w for w in withouts if w in content]
    assert not leaked, f"{txt.name} unexpectedly contains: {leaked}"
