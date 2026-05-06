"""Runner registry. See benchmarks/README.md for the three-axis philosophy."""
from __future__ import annotations

import os
import re
import subprocess
import sys
from collections.abc import Iterable
from dataclasses import dataclass, field
from shlex import quote
from typing import Literal

from .config import Config

RunnerKind = Literal["speed", "extract"]
OutputFormat = Literal["plain", "html"]

RUN_TIMEOUT = 120  # seconds; covers Chromium cold-start on CI
_URL_SEP = "\n\f\n"

EXTRACTION_FIXTURES: tuple[str, ...] = (
    "article-footer-heavy",
    "documentation-sidebar",
    "service-multi-section",
    "forum-thread",
    "product-jsonld",
    "collection-grid",
    "listing-cards",
)


@dataclass(frozen=True, slots=True)
class Runner:
    label: str
    argv_prefix: list[str]
    env: dict[str, str] = field(default_factory=dict)
    kind: RunnerKind = "speed"
    required: bool = True
    # One URL per invocation for CLIs like chrome-headless-shell --dump-dom.
    single_url: bool = False
    output_format: OutputFormat = "plain"

    @property
    def slug(self) -> str:
        """Filename-safe label (alnum + dashes)."""
        return re.sub(r"[^A-Za-z0-9]+", "-", self.label).strip("-")

    def cmd(self, urls: Iterable[str]) -> list[str]:
        return [*self.argv_prefix, *urls]

    def run(self, urls: Iterable[str]) -> str:
        """Execute synchronously, return post-processed stdout."""
        urls_list = list(urls)
        if self.single_url and len(urls_list) > 1:
            return _URL_SEP.join(self._run_one([u]) for u in urls_list) + "\n"
        return self._run_one(urls_list)

    def _run_one(self, urls: list[str]) -> str:
        try:
            proc = subprocess.run(
                self.cmd(urls),
                capture_output=True, text=True, check=False,
                env={**os.environ, **self.env},
                timeout=RUN_TIMEOUT,
            )
        except subprocess.TimeoutExpired:
            print(f"[{self.label}] timed out after {RUN_TIMEOUT}s", file=sys.stderr)
            return ""
        if proc.returncode != 0 and proc.stderr:
            sys.stderr.write(proc.stderr)
        if self.output_format == "html":
            return _strip_inline_text(proc.stdout)
        return proc.stdout

    def shell(self, urls: Iterable[str]) -> str:
        """Render as one string for `hyperfine --shell=none` (engine time only)."""
        env_part = " ".join(f"{k}={quote(v)}" for k, v in self.env.items())
        argv = " ".join(quote(a) for a in self.argv_prefix)
        urls_quoted = " ".join(quote(u) for u in urls)
        prefix = f"env {env_part} " if env_part else ""
        return f"{prefix}{argv} {urls_quoted}".strip()


def _node_env(cfg: Config) -> dict[str, str]:
    return {"NODE_PATH": cfg.node_path()}


def speed_runners(cfg: Config) -> list[Runner]:
    """Text-output runners for equivalence / time / memory / parallel."""
    runners: list[Runner] = [
        Runner("servo-fetch", [str(cfg.servo_fetch_bin), "--raw", "text", "-q"]),
    ]
    if _has_chrome_headless_shell(cfg):
        runners.append(Runner(
            "chrome-headless-shell",
            [str(cfg.chrome_headless_shell_bin), "--headless=new", "--dump-dom"],
            required=False,
            single_url=True,
            output_format="html",
        ))
    if _has_lightpanda(cfg):
        runners.append(Runner(
            "lightpanda",
            [str(cfg.lightpanda_bin), "fetch"],
            required=False,
            single_url=True,
            output_format="html",
        ))
    runners.append(Runner(
        "playwright:optimized",
        [cfg.node_bin, str(cfg.tools_dir / "playwright-runner.js")],
        env=_node_env(cfg),
    ))
    return runners


def extract_runners(cfg: Config) -> list[Runner]:
    """Extraction-quality runners (servo-fetch vs DOM-only baseline)."""
    return [
        Runner("servo-fetch (layout-aware)",
               [str(cfg.servo_fetch_bin), "-q"], kind="extract"),
        Runner("Readability (DOM-only)",
               [cfg.node_bin, str(cfg.tools_dir / "readability-runner.js")],
               env=_node_env(cfg), kind="extract"),
    ]


def curl_baseline(urls: Iterable[str]) -> str:
    """Pure-HTTP baseline: raw fetch + naive tag strip. No JS execution."""
    parts: list[str] = []
    for url in urls:
        try:
            out = subprocess.check_output(
                ["curl", "-sf", "--max-time", "30", url], text=True, timeout=60,
            )
            parts.append(_strip_inline_text(out))
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired, FileNotFoundError):
            parts.append("")
    return _URL_SEP.join(parts) + "\n"


def _strip_inline_text(html: str) -> str:
    h = re.sub(r"<script[\s\S]*?</script>", "", html, flags=re.I)
    h = re.sub(r"<style[\s\S]*?</style>", "", h, flags=re.I)
    h = re.sub(r"<[^>]+>", "", h)
    h = re.sub(r"[ \t]+", " ", h)
    return re.sub(r"\n\s*\n\s*", "\n\n", h).strip()


def _has_chrome_headless_shell(cfg: Config) -> bool:
    return cfg.chrome_headless_shell_bin.is_file() and os.access(
        cfg.chrome_headless_shell_bin, os.X_OK,
    )


def _has_lightpanda(cfg: Config) -> bool:
    return cfg.lightpanda_bin.is_file() and os.access(cfg.lightpanda_bin, os.X_OK)
