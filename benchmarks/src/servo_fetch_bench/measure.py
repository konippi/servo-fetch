"""Peak resident memory measurement (psutil tree default; `cgmemtime` / legacy `time` opt-in)."""
from __future__ import annotations

import contextlib
import os
import platform
import re
import shutil
import signal
import subprocess
import time

import psutil

_MAC_PEAK = re.compile(r"^\s*(\d+)\s+peak memory footprint", re.MULTILINE)
_MAC_RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size", re.MULTILINE)
_GNU_RSS = re.compile(r"Maximum resident set size.*?:\s*(\d+)")
_CGMEMTIME_GROUP = re.compile(r"Group high-water RSS:\s*(\d+)\s*KiB")
_CGMEMTIME_CHILD = re.compile(r"Child high-water RSS:\s*(\d+)\s*KiB")

_MEASURE_TIMEOUT = 180  # per-invocation ceiling; Chromium cold-start can reach ~5s.
_TREE_POLL_INTERVAL = 0.01  # 10 ms matches psrecord / cgmemtime default cadence.


def peak_bytes(
    cmd: list[str],
    *,
    env: dict[str, str] | None = None,
    use_cgmemtime: bool = False,
    tree: bool = True,
) -> int:
    """Run `cmd`, return peak resident bytes. 0 on parse failure."""
    if use_cgmemtime and shutil.which("cgmemtime"):
        return _parse_cgmemtime(cmd, env)
    if tree:
        return _peak_tree(cmd, env)
    return _peak_legacy(cmd, env)


def _merge_env(env: dict[str, str] | None) -> dict[str, str] | None:
    """Overlay `env` onto os.environ; None means inherit parent's env."""
    return {**os.environ, **env} if env else None


def _kill_group(proc: subprocess.Popen[bytes]) -> None:
    """SIGKILL the session led by `proc`."""
    with contextlib.suppress(ProcessLookupError, PermissionError, OSError):
        os.killpg(proc.pid, signal.SIGKILL)


def _peak_tree(cmd: list[str], env: dict[str, str] | None) -> int:
    """Poll the process tree at 10ms intervals; return peak combined RSS."""
    try:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            start_new_session=True,  # leader pid == pgid; see _kill_group
            env=_merge_env(env),
        )
    except OSError:
        return 0

    peak = 0
    deadline = time.monotonic() + _MEASURE_TIMEOUT
    try:
        parent = psutil.Process(proc.pid)
        peak = _tree_rss(parent)  # initial snapshot catches short-lived children
        while proc.poll() is None:
            if time.monotonic() > deadline:
                _kill_group(proc)
                break
            peak = max(peak, _tree_rss(parent))
            time.sleep(_TREE_POLL_INTERVAL)
    except psutil.NoSuchProcess:
        pass
    finally:
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            _kill_group(proc)
            with contextlib.suppress(subprocess.TimeoutExpired):
                proc.wait(timeout=5)
    return peak


def _tree_rss(parent: psutil.Process) -> int:
    """Sum RSS of `parent` + all descendants, robust to races."""
    total = 0
    try:
        procs = [parent, *parent.children(recursive=True)]
    except psutil.NoSuchProcess:
        return 0
    for p in procs:
        try:
            total += p.memory_info().rss
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue  # child died mid-sample or perms revoked — best-effort
    return total


def _peak_legacy(cmd: list[str], env: dict[str, str] | None) -> int:
    if platform.system() == "Darwin":
        return _parse_macos(cmd, env)
    return _parse_gnu_time(cmd, env)


def _capture_stderr(argv: list[str], env: dict[str, str] | None = None) -> str:
    try:
        proc = subprocess.run(
            argv, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            text=True, check=False, timeout=_MEASURE_TIMEOUT, env=env,
        )
    except subprocess.TimeoutExpired:
        return ""
    return proc.stderr


def _parse_macos(cmd: list[str], env: dict[str, str] | None) -> int:
    out = _capture_stderr(["/usr/bin/time", "-l", *cmd], _merge_env(env))
    m = _MAC_PEAK.search(out) or _MAC_RSS.search(out)
    return int(m.group(1)) if m else 0


def _parse_gnu_time(cmd: list[str], env: dict[str, str] | None) -> int:
    out = _capture_stderr(["/usr/bin/time", "-v", *cmd], _merge_env(env))
    m = _GNU_RSS.search(out)
    # GNU time reports max RSS in KiB; convert to bytes.
    return int(m.group(1)) * 1024 if m else 0


def _parse_cgmemtime(cmd: list[str], env: dict[str, str] | None) -> int:
    try:
        proc = subprocess.run(
            ["cgmemtime", "-t", *cmd], capture_output=True, text=True,
            check=False, timeout=_MEASURE_TIMEOUT, env=_merge_env(env),
        )
    except subprocess.TimeoutExpired:
        return 0
    # Prefer group (whole cgroup) high-water; fall back to child-only.
    m = _CGMEMTIME_GROUP.search(proc.stderr) or _CGMEMTIME_CHILD.search(proc.stderr)
    return int(m.group(1)) * 1024 if m else 0


def human(n: int) -> str:
    """Render `n` bytes as GB/MB/KB/B with at most 2 significant decimals."""
    for unit, step, digits in (("GB", 1 << 30, 2), ("MB", 1 << 20, 1), ("KB", 1 << 10, 1)):
        if n >= step:
            return f"{n / step:.{digits}f} {unit}"
    return f"{n} B"
