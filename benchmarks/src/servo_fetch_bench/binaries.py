"""Download optional peer binaries (chrome-headless-shell + lightpanda)."""
from __future__ import annotations

import platform
import shutil
import tempfile
import urllib.request
import zipfile
from pathlib import Path
from typing import Literal

# Pinned versions. Benchmark numbers depend on these — bump with care and
# regenerate `published-*` results when changing.
LIGHTPANDA_TAG = "nightly"
CHROME_VERSION = "148.0.7778.96"

# Streaming chunk for large downloads (Chrome zip is ~170 MB).
_CHUNK_SIZE = 1 << 20  # 1 MiB

# Total wall-clock timeout per HTTP request.
_DOWNLOAD_TIMEOUT = 300


def _platform() -> tuple[str, str]:
    system = platform.system().lower()
    machine = platform.machine().lower()
    if system not in {"darwin", "linux"}:
        msg = f"Unsupported OS for optional binaries: {system}"
        raise RuntimeError(msg)
    arch = {
        "x86_64": "x86_64", "amd64": "x86_64",
        "arm64": "aarch64", "aarch64": "aarch64",
    }.get(machine)
    if arch is None:
        msg = f"Unsupported architecture: {machine}"
        raise RuntimeError(msg)
    return system, arch


def _chrome_platform() -> str:
    """Chrome for Testing platform key (e.g., 'mac-arm64', 'linux64')."""
    system, arch = _platform()
    return {
        ("darwin", "aarch64"): "mac-arm64",
        ("darwin", "x86_64"): "mac-x64",
        ("linux", "x86_64"): "linux64",
    }[(system, arch)]


def chrome_headless_shell_path(bin_dir: Path) -> Path:
    """Canonical path to the installed chrome-headless-shell binary."""
    return bin_dir / f"chrome-headless-shell-{_chrome_platform()}" / "chrome-headless-shell"


def _lightpanda_url() -> str:
    system, arch = _platform()
    os_suffix = {"darwin": "macos", "linux": "linux"}[system]
    return (
        f"https://github.com/lightpanda-io/browser/releases/download/"
        f"{LIGHTPANDA_TAG}/lightpanda-{arch}-{os_suffix}"
    )


def _chrome_headless_shell_url() -> str:
    platform_key = _chrome_platform()
    return (
        f"https://storage.googleapis.com/chrome-for-testing-public/"
        f"{CHROME_VERSION}/{platform_key}/chrome-headless-shell-{platform_key}.zip"
    )


def _stream_download(url: str, dest: Path) -> None:
    """Stream-download `url` to `dest`, 1 MiB at a time."""
    with urllib.request.urlopen(url, timeout=_DOWNLOAD_TIMEOUT) as resp, \
            dest.open("wb") as f:
        shutil.copyfileobj(resp, f, length=_CHUNK_SIZE)


def _safe_extract(zf: zipfile.ZipFile, target: Path) -> None:
    """Extract `zf` into `target` with Zip Slip protection and mode preservation."""
    target_resolved = target.resolve()
    for info in zf.infolist():
        dest = (target / info.filename).resolve()
        if target_resolved not in dest.parents and dest != target_resolved:
            msg = f"Zip Slip rejected: {info.filename!r} escapes {target_resolved}"
            raise RuntimeError(msg)

    zf.extractall(target)
    for info in zf.infolist():
        if info.is_dir():
            continue
        mode = (info.external_attr >> 16) & 0o777
        if mode:
            (target / info.filename).chmod(mode)


def install_lightpanda(bin_dir: Path) -> Path:
    """Install the Lightpanda nightly binary at `bin_dir/lightpanda`."""
    bin_dir.mkdir(parents=True, exist_ok=True)
    target = bin_dir / "lightpanda"

    with tempfile.NamedTemporaryFile(dir=bin_dir, prefix=".lightpanda.", delete=False) as f:
        tmp = Path(f.name)
    try:
        _stream_download(_lightpanda_url(), tmp)
        tmp.chmod(0o755)  # standard Unix convention for installed executables
        tmp.replace(target)
    except BaseException:
        tmp.unlink(missing_ok=True)
        raise
    return target


def install_chrome_headless_shell(bin_dir: Path) -> Path:
    """Install chrome-headless-shell from the Chrome for Testing public bucket."""
    bin_dir.mkdir(parents=True, exist_ok=True)
    platform_key = _chrome_platform()
    binary = chrome_headless_shell_path(bin_dir)
    target_dir = binary.parent

    with tempfile.TemporaryDirectory(dir=bin_dir, prefix=".chs.") as tmp_str:
        tmp = Path(tmp_str)
        zip_path = tmp / "archive.zip"
        _stream_download(_chrome_headless_shell_url(), zip_path)
        extract_dir = tmp / "extract"
        with zipfile.ZipFile(zip_path) as zf:
            _safe_extract(zf, extract_dir)
        staged = extract_dir / f"chrome-headless-shell-{platform_key}"
        if not (staged / "chrome-headless-shell").is_file():
            msg = f"Unexpected archive layout: missing {staged}/chrome-headless-shell"
            raise RuntimeError(msg)
        if target_dir.exists():
            shutil.rmtree(target_dir)
        shutil.move(str(staged), str(target_dir))
    binary.chmod(0o755)
    return binary


InstallTarget = Literal["lightpanda", "chrome-headless-shell", "all"]


def install(bin_dir: Path, which: InstallTarget) -> None:
    if which in {"lightpanda", "all"}:
        print(f"installing lightpanda → {bin_dir / 'lightpanda'}")
        install_lightpanda(bin_dir)
    if which in {"chrome-headless-shell", "all"}:
        binary = chrome_headless_shell_path(bin_dir)
        print(f"installing chrome-headless-shell → {binary}")
        install_chrome_headless_shell(bin_dir)
