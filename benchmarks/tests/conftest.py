"""Shared test fixtures."""
from __future__ import annotations

import socket
from collections.abc import Callable
from pathlib import Path

import pytest
from typer.testing import CliRunner

from servo_fetch_bench.config import Config

_ENV_VARS = (
    "PORT", "WARMUP", "MIN_RUNS", "MEMORY_RUNS", "FIXTURES",
    "PARALLEL_URLS", "BENCH_HOST", "NODE_BIN", "SERVO_FETCH_BIN",
    "CHROME_HEADLESS_SHELL_BIN", "LIGHTPANDA_BIN",
)


@pytest.fixture
def clean_env(monkeypatch: pytest.MonkeyPatch) -> None:
    for k in _ENV_VARS:
        monkeypatch.delenv(k, raising=False)


@pytest.fixture
def cfg(clean_env: None) -> Config:
    return Config()


@pytest.fixture
def fixtures_dir(cfg: Config) -> Path:
    return cfg.fixtures_dir


@pytest.fixture
def cli_runner() -> CliRunner:
    return CliRunner()


@pytest.fixture
def make_stub_bin(tmp_path: Path) -> Callable[..., Path]:
    """Factory: create a stub file at tmp_path/<name> (executable by default)."""
    def _make(name: str, *, content: bytes = b"", executable: bool = True) -> Path:
        path = tmp_path / name
        path.write_bytes(content)
        if executable:
            path.chmod(0o755)
        return path
    return _make


@pytest.fixture
def free_port() -> int:
    """Grab an unused TCP port on 127.0.0.1 (bind-and-release trick)."""
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
