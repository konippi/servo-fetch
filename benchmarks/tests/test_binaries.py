"""Unit tests for optional-binary download helpers (no network)."""
from __future__ import annotations

from collections.abc import Callable

import pytest

from servo_fetch_bench import binaries


@pytest.fixture
def fake_platform(monkeypatch: pytest.MonkeyPatch) -> Callable[[str, str], None]:
    def _set(system: str, machine: str) -> None:
        monkeypatch.setattr(binaries.platform, "system", lambda: system)
        monkeypatch.setattr(binaries.platform, "machine", lambda: machine)
    return _set


@pytest.mark.parametrize(
    ("system", "machine", "expected"),
    [
        ("Darwin", "arm64", ("darwin", "aarch64")),
        ("Darwin", "x86_64", ("darwin", "x86_64")),
        ("Linux", "x86_64", ("linux", "x86_64")),
        ("Linux", "aarch64", ("linux", "aarch64")),
        ("Linux", "amd64", ("linux", "x86_64")),
    ],
)
def test_platform_supported(
    fake_platform: Callable[[str, str], None],
    system: str,
    machine: str,
    expected: tuple[str, str],
) -> None:
    fake_platform(system, machine)
    assert binaries._platform() == expected


@pytest.mark.parametrize(
    ("system", "machine"),
    [("Windows", "x86_64"), ("Darwin", "ppc"), ("Linux", "mips")],
)
def test_platform_rejects_unsupported(
    fake_platform: Callable[[str, str], None], system: str, machine: str,
) -> None:
    fake_platform(system, machine)
    with pytest.raises(RuntimeError, match="Unsupported"):
        binaries._platform()


@pytest.mark.parametrize(
    ("system", "machine", "expected_filename"),
    [
        ("Darwin", "arm64", "lightpanda-aarch64-macos"),
        ("Linux", "x86_64", "lightpanda-x86_64-linux"),
        ("Linux", "aarch64", "lightpanda-aarch64-linux"),
    ],
)
def test_lightpanda_url_matches_release_naming(
    fake_platform: Callable[[str, str], None],
    system: str,
    machine: str,
    expected_filename: str,
) -> None:
    fake_platform(system, machine)
    url = binaries._lightpanda_url()
    assert url.startswith("https://github.com/lightpanda-io/browser/releases/download/")
    assert url.endswith(expected_filename)


@pytest.mark.parametrize(
    ("system", "machine", "expected"),
    [
        ("Darwin", "arm64", "mac-arm64"),
        ("Linux", "x86_64", "linux64"),
    ],
)
def test_chrome_platform_maps_system_to_cft_string(
    fake_platform: Callable[[str, str], None],
    system: str,
    machine: str,
    expected: str,
) -> None:
    fake_platform(system, machine)
    assert binaries._chrome_platform() == expected
