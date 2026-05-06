"""Benchmark configuration, loaded from environment variables."""
from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

from .binaries import chrome_headless_shell_path

BENCH_DIR = Path(__file__).resolve().parent.parent.parent


def _chrome_default_path() -> Path:
    """Default chrome-headless-shell path, honoring env override."""
    if env := os.environ.get("CHROME_HEADLESS_SHELL_BIN"):
        return Path(env)
    try:
        return chrome_headless_shell_path(BENCH_DIR / "bin")
    except RuntimeError:
        return BENCH_DIR / "bin" / "chrome-headless-shell"  # unreachable binary; will be skipped


@dataclass(frozen=True, slots=True)
class Config:
    port: int = field(
        default_factory=lambda: int(os.environ.get("PORT") or 8731),
    )
    bench_host: str = field(
        default_factory=lambda: os.environ.get("BENCH_HOST") or "bench.servo-fetch",
    )
    warmup: int = field(
        default_factory=lambda: int(os.environ.get("WARMUP") or 3),
    )
    min_runs: int = field(
        default_factory=lambda: int(os.environ.get("MIN_RUNS") or 10),
    )
    memory_runs: int = field(
        default_factory=lambda: int(os.environ.get("MEMORY_RUNS") or 5),
    )
    parallel_urls: tuple[int, ...] = field(
        default_factory=lambda: tuple(
            int(n) for n in (os.environ.get("PARALLEL_URLS") or "1,2,4,8").split(",")
        ),
    )
    fixtures: tuple[str, ...] = field(
        default_factory=lambda: tuple(
            (os.environ.get("FIXTURES") or "static-small spa-light spa-heavy").split(),
        ),
    )
    servo_fetch_bin: Path = field(
        default_factory=lambda: Path(
            os.environ.get("SERVO_FETCH_BIN")
            or BENCH_DIR.parent / "target/release/servo-fetch",
        ),
    )
    node_bin: str = field(
        default_factory=lambda: os.environ.get("NODE_BIN") or "node",
    )
    chrome_headless_shell_bin: Path = field(
        default_factory=_chrome_default_path,
    )
    lightpanda_bin: Path = field(
        default_factory=lambda: Path(
            os.environ.get("LIGHTPANDA_BIN") or BENCH_DIR / "bin/lightpanda",
        ),
    )

    @property
    def results_dir(self) -> Path:
        return BENCH_DIR / "results"

    @property
    def fixtures_dir(self) -> Path:
        return BENCH_DIR / "fixtures"

    @property
    def tools_dir(self) -> Path:
        return BENCH_DIR / "tools"

    @property
    def bin_dir(self) -> Path:
        return BENCH_DIR / "bin"

    def fixture_url(self, name: str, subpath: str = "") -> str:
        prefix = f"/{subpath}" if subpath else ""
        return f"http://{self.bench_host}:{self.port}{prefix}/{name}.html"

    def node_path(self) -> str:
        """`NODE_PATH` env-var value (string, per POSIX env semantics)."""
        return str(BENCH_DIR / "node" / "node_modules")
