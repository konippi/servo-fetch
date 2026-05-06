"""Pre-flight sanity checks: declared paths + runner binaries resolve."""
from __future__ import annotations

import shutil
from collections.abc import Callable
from pathlib import Path

import pytest

from servo_fetch_bench.config import Config
from servo_fetch_bench.runners import EXTRACTION_FIXTURES, Runner, extract_runners, speed_runners


def test_speed_fixtures_have_html_and_expect_files(cfg: Config) -> None:
    for name in cfg.fixtures:
        assert (cfg.fixtures_dir / "perf" / f"{name}.html").is_file(), name
        assert (cfg.fixtures_dir / "perf" / "golden" / f"{name}.expect").is_file(), name


@pytest.mark.parametrize("name", EXTRACTION_FIXTURES)
def test_page_type_fixture_has_html_txt_and_expect(cfg: Config, name: str) -> None:
    lc = cfg.fixtures_dir / "extraction"
    assert (lc / f"{name}.html").is_file()
    assert (lc / "golden" / f"{name}.txt").is_file()
    assert (lc / "golden" / f"{name}.expect").is_file()


@pytest.mark.parametrize(
    "factory", [speed_runners, extract_runners],
    ids=["speed", "extract"],
)
def test_runner_executables_resolve_or_skip(
    cfg: Config, factory: Callable[[Config], list[Runner]],
) -> None:
    for runner in factory(cfg):
        exe = runner.argv_prefix[0]
        resolved = Path(exe).is_file() if Path(exe).is_absolute() else shutil.which(exe)
        if Path(exe).is_absolute() and not resolved:
            pytest.skip(f"{runner.label}: {exe} not built")
        assert resolved, f"{runner.label}: {exe} not found"
