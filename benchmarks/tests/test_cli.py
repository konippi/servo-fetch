"""CLI smoke tests via Typer's CliRunner."""
from __future__ import annotations

import pytest
from typer.testing import CliRunner

from servo_fetch_bench.cli import app


def test_help_lists_harness_summary(cli_runner: CliRunner) -> None:
    result = cli_runner.invoke(app, ["--help"])
    assert result.exit_code == 0
    assert "servo-fetch benchmark harness" in result.output


def test_extract_group_lists_subcommands(cli_runner: CliRunner) -> None:
    result = cli_runner.invoke(app, ["extract", "--help"])
    assert result.exit_code == 0
    assert "layout" in result.output
    assert "dataset" in result.output


@pytest.mark.parametrize(
    "argv", [["download", "bogus"], ["extract", "dataset", "bogus"]],
)
def test_invalid_dataset_exits_nonzero(cli_runner: CliRunner, argv: list[str]) -> None:
    assert cli_runner.invoke(app, argv).exit_code != 0


@pytest.mark.parametrize(
    "cmd",
    [
        "setup", "install-binaries", "download", "equivalence",
        "time", "parallel", "memory", "size", "all", "extract",
    ],
)
def test_subcommand_help_exits_zero(cli_runner: CliRunner, cmd: str) -> None:
    result = cli_runner.invoke(app, [cmd, "--help"])
    assert result.exit_code == 0
