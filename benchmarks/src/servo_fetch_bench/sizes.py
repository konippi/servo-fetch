"""Binary footprint measurement (size + non-system dylib count)."""
from __future__ import annotations

import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

from .config import Config


@dataclass(frozen=True, slots=True)
class SizeEntry:
    label: str
    binary: Path | None
    size_bytes: int | None
    non_system_deps: int | None

    @property
    def size_mib(self) -> float | None:
        return None if self.size_bytes is None else self.size_bytes / (1024 * 1024)


# Paths rooted here ship on every macOS / Linux install; don't count as deps.
_SYSTEM_LIB_PREFIXES = ("/System/Library/", "/usr/lib/", "/lib/", "/lib64/")


def binary_targets(cfg: Config) -> list[tuple[str, Path | None]]:
    """(label, resolved path) for each comparable tool."""
    def _opt(p: Path) -> Path | None:
        return p if p.is_file() else None

    node = shutil.which(cfg.node_bin)
    curl = shutil.which("curl")
    return [
        ("servo-fetch",           _opt(cfg.servo_fetch_bin)),
        ("chrome-headless-shell", _opt(cfg.chrome_headless_shell_bin)),
        ("lightpanda",            _opt(cfg.lightpanda_bin)),
        ("curl",                  Path(curl) if curl else None),
        ("node",                  Path(node) if node else None),
    ]


def non_system_deps(path: Path) -> int | None:
    """Count non-stdlib dynamic deps via `otool -L` (macOS) or `ldd` (Linux); None if unavailable."""
    tool = shutil.which("otool") or shutil.which("ldd")
    if tool is None:
        return None
    argv = [tool, "-L", str(path)] if Path(tool).name == "otool" else [tool, str(path)]
    try:
        out = subprocess.check_output(argv, text=True, timeout=10, stderr=subprocess.DEVNULL)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, FileNotFoundError):
        return None
    return sum(1 for line in out.splitlines() if _is_non_system_dep(line))


def _is_non_system_dep(line: str) -> bool:
    """True iff this otool/ldd line names a non-system shared-library dep."""
    first = line.strip().split(" ", 1)[0] if line.strip() else ""
    # otool header ("path:"), ldd virtual-lib markers ("(0x...)"), and
    # kernel-provided entries without absolute paths (linux-vdso etc.) don't count.
    if not first or first.endswith(":") or first.startswith("(") or not first.startswith("/"):
        return False
    return not any(first.startswith(p) for p in _SYSTEM_LIB_PREFIXES)


def measure(cfg: Config) -> list[SizeEntry]:
    """Measure every target. Missing binaries yield a SizeEntry with None fields."""
    entries: list[SizeEntry] = []
    for label, path in binary_targets(cfg):
        if path is None:
            entries.append(SizeEntry(label, None, None, None))
            continue
        entries.append(SizeEntry(
            label=label,
            binary=path,
            size_bytes=path.stat().st_size,
            non_system_deps=non_system_deps(path),
        ))
    return entries
