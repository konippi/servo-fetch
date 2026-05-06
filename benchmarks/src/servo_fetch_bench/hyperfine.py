"""Wrapper around the `hyperfine` CLI binary."""
from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import typer


def require() -> None:
    if shutil.which("hyperfine") is None:
        raise typer.BadParameter(
            "hyperfine is required for timing benchmarks.\n"
            "  macOS: brew install hyperfine\n"
            "  Linux: apt install hyperfine  (or see https://github.com/sharkdp/hyperfine)",
        )


def run(
    commands: list[tuple[str, str]],
    *,
    warmup: int,
    min_runs: int,
    export_md: Path | None = None,
    export_json: Path | None = None,
) -> None:
    args = [
        "hyperfine",
        "--warmup", str(warmup),
        "--min-runs", str(min_runs),
        "--shell=none",
        "--ignore-failure",
    ]
    if export_md:
        args += ["--export-markdown", str(export_md)]
    if export_json:
        args += ["--export-json", str(export_json)]
    for name, cmd in commands:
        args += ["--command-name", name, cmd]
    subprocess.run(args, check=True, timeout=900)
