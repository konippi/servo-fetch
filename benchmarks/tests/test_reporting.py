"""Smoke tests for report generation."""
from __future__ import annotations

import pytest

from servo_fetch_bench.config import Config
from servo_fetch_bench.reporting import _tool_version, env_header, md_table


def test_env_header_contains_required_fields(cfg: Config) -> None:
    header = env_header(cfg)
    for field in ("date", "host OS", "arch", "CPU", "hyperfine", "node", "servo-fetch"):
        assert field in header


def test_md_table_renders_basic_two_column_structure() -> None:
    out = md_table(["A", "B"], [["1", "2"], ["3", "4"]])
    assert out.splitlines() == [
        "| A | B |",
        "|---|---|",
        "| 1 | 2 |",
        "| 3 | 4 |",
    ]


def test_md_table_applies_custom_alignment_row() -> None:
    out = md_table(["A", "B"], [["1", "2"]], aligns=["-", "-:"])
    assert out.splitlines()[1] == "|-|-:|"


def test_md_table_renders_header_only_when_rows_empty() -> None:
    out = md_table(["A", "B"], [])
    assert out.splitlines() == ["| A | B |", "|---|---|"]


@pytest.mark.parametrize("argv", [["echo", "x"], ["/bin/sh", "-c", "true"]])
def test_tool_version_returns_string_for_known_tool(argv: list[str]) -> None:
    assert isinstance(_tool_version(argv), str)


def test_tool_version_reports_not_installed_for_missing_tool() -> None:
    assert _tool_version(["/__nonexistent__/tool", "--version"]) == "not installed"
