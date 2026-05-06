"""/etc/hosts alias installer (see benchmarks/README.md for rationale)."""
from __future__ import annotations

import re
import subprocess
from pathlib import Path

import typer
from rich.console import Console

HOSTS_FILE = Path("/etc/hosts")

_console = Console(stderr=True)


def is_configured(hostname: str) -> bool:
    if not HOSTS_FILE.exists():
        return False
    pattern = re.compile(rf"^\s*127\.0\.0\.1\s+{re.escape(hostname)}(\s|$)", re.MULTILINE)
    return bool(pattern.search(HOSTS_FILE.read_text(encoding="utf-8")))


def install(hostname: str) -> None:
    """Append the alias via `sudo tee -a`. Idempotent."""
    if is_configured(hostname):
        _console.print(f"alias '[cyan]{hostname}[/]' already configured")
        return
    line = f"127.0.0.1 {hostname}\n"
    _console.print(f"Appending '[yellow]{line.strip()}[/]' to /etc/hosts (sudo required).")
    try:
        proc = subprocess.run(
            ["sudo", "tee", "-a", str(HOSTS_FILE)],
            input=line, text=True, capture_output=True, check=False, timeout=60,
        )
    except subprocess.TimeoutExpired as e:
        raise typer.BadParameter("sudo tee timed out (password prompt?)") from e
    if proc.returncode != 0:
        raise typer.BadParameter(f"sudo tee failed: {proc.stderr.strip()}")


def require(hostname: str) -> None:
    """Raise if the alias is missing, with a fix-it hint."""
    if not is_configured(hostname):
        raise typer.BadParameter(
            f"hostname '{hostname}' is not in /etc/hosts.\n"
            "run once:  ./benchmarks/bench setup\n"
            "(servo-fetch blocks 127.0.0.0/8 by design; the alias is a "
            "loopback-backed workaround that keeps production code untouched.)",
        )
