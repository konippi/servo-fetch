"""Report header + table formatting."""
from __future__ import annotations

import json
import platform
import shutil
import subprocess
from datetime import UTC, datetime
from pathlib import Path

import psutil

from .config import Config


def env_header(cfg: Config) -> str:
    """Reproducibility header attached to every result file."""
    mem_gb = psutil.virtual_memory().total / (1024**3)
    cpu_p = psutil.cpu_count(logical=False) or 0
    cpu_l = psutil.cpu_count(logical=True) or 0
    pairs: list[tuple[str, str]] = [
        ("date", datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")),
        ("host OS", _host_os()),
        ("arch", platform.machine()),
        ("CPU", f"{_cpu_brand()} ({cpu_p}P / {cpu_l}L)"),
        ("memory", f"{mem_gb:.1f} GiB"),
        ("hyperfine", _tool_version(["hyperfine", "--version"])),
        ("node", _tool_version([cfg.node_bin, "--version"])),
        ("servo-fetch", _tool_version([str(cfg.servo_fetch_bin), "--version"])),
        ("Playwright", _playwright_version(cfg)),
        ("chrome-headless-shell",
            _tool_version([str(cfg.chrome_headless_shell_bin), "--version"])),
        ("lightpanda", _tool_version([str(cfg.lightpanda_bin), "--version"])),
    ]
    key_w = max(len(k) for k, _ in pairs) + 1
    body = "\n".join(f"- {k:<{key_w}}: {v}" for k, v in pairs)
    return f"# Environment\n{body}\n\n"


def _host_os() -> str:
    """Human-readable OS identifier: macOS product version, Linux distro PRETTY_NAME, or uname fallback."""
    system = platform.system()
    release = platform.release()
    if system == "Darwin":
        mac_ver = platform.mac_ver()[0]
        return f"macOS {mac_ver} (darwin {release})" if mac_ver else f"darwin ({release})"
    if system == "Linux":
        try:
            info = platform.freedesktop_os_release()
        except (OSError, AttributeError):
            return f"linux ({release})"
        name = info.get("PRETTY_NAME") or info.get("NAME")
        return f"{name} (linux {release})" if name else f"linux ({release})"
    return f"{system.lower()} ({release})"


def md_table(
    headers: list[str],
    rows: list[list[str]],
    aligns: list[str] | None = None,
) -> str:
    """Build a compact Markdown table. `aligns`: '-' left, '-:' right, ':-:' center."""
    al = aligns or ["---"] * len(headers)
    out = ["| " + " | ".join(headers) + " |", "|" + "|".join(al) + "|"]
    out += ["| " + " | ".join(r) + " |" for r in rows]
    return "\n".join(out) + "\n"


def _cpu_brand() -> str:
    """CPU model name via `sysctl` (macOS) or `/proc/cpuinfo` (Linux); 'unknown' on failure."""
    if platform.system() == "Darwin":
        try:
            return subprocess.check_output(
                ["sysctl", "-n", "machdep.cpu.brand_string"], text=True, timeout=5,
            ).strip()
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            return "unknown"
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        for line in cpuinfo.read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    return "unknown"


def _playwright_version(cfg: Config) -> str:
    pkg = Path(cfg.node_path()) / "playwright" / "package.json"
    if not pkg.exists():
        return "not installed"
    try:
        return json.loads(pkg.read_text(encoding="utf-8")).get("version", "unknown")
    except (json.JSONDecodeError, OSError):
        return "unknown"


def _tool_version(cmd: list[str]) -> str:
    if shutil.which(cmd[0]) is None and not Path(cmd[0]).exists():
        return "not installed"
    try:
        out = subprocess.check_output(
            cmd, text=True, stderr=subprocess.STDOUT, timeout=5,
        ).splitlines()
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return "unknown"
    return out[0] if out else "unknown"
