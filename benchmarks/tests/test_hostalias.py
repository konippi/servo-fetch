"""Regression tests for hostalias.is_configured + install."""
from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

import pytest
import typer

from servo_fetch_bench import hostalias


@pytest.fixture
def hosts_file(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    path = tmp_path / "hosts"
    monkeypatch.setattr(hostalias, "HOSTS_FILE", path)
    return path


@pytest.mark.parametrize(
    ("contents", "expected"),
    [
        ("127.0.0.1 bench.servo-fetch\n", True),
        ("127.0.0.1\tbench.servo-fetch\n", True),
        ("   127.0.0.1 bench.servo-fetch\n", True),
        ("127.0.0.1 bench.servo-fetch # alias\n", True),
        ("# 127.0.0.1 bench.servo-fetch\n", False),
        ("#127.0.0.1 bench.servo-fetch\n", False),
        ("  # 127.0.0.1 bench.servo-fetch\n", False),
        ("## 127.0.0.1 bench.servo-fetch\n", False),
        ("127.0.0.10 bench.servo-fetch\n", False),
        ("127.0.0.1 bench.servo-fetch-other\n", False),
        ("", False),
    ],
    ids=[
        "plain", "tab", "leading-space", "trailing-comment",
        "comment-with-space", "comment-no-space", "ws-then-hash", "double-hash",
        "prefix-mismatch", "hostname-suffix-mismatch", "empty",
    ],
)
def test_is_configured(hosts_file: Path, contents: str, expected: bool) -> None:
    hosts_file.write_text(contents)
    assert hostalias.is_configured("bench.servo-fetch") is expected


def test_is_configured_returns_false_when_file_missing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(hostalias, "HOSTS_FILE", tmp_path / "nonexistent")
    assert hostalias.is_configured("bench.servo-fetch") is False


def test_install_skips_when_already_configured(
    hosts_file: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    hosts_file.write_text("127.0.0.1 already.there\n")
    calls: list = []
    monkeypatch.setattr(hostalias.subprocess, "run", lambda *a, **k: calls.append(a))
    hostalias.install("already.there")
    assert calls == []


def test_install_appends_alias_via_sudo_tee(
    hosts_file: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    hosts_file.write_text("# comment\n")

    def fake_run(cmd, *, input, **_kwargs):
        hosts_file.write_text(hosts_file.read_text() + input)
        return SimpleNamespace(returncode=0, stderr="")

    monkeypatch.setattr(hostalias.subprocess, "run", fake_run)
    hostalias.install("new.host")
    assert "127.0.0.1 new.host" in hosts_file.read_text()


def test_install_raises_on_sudo_failure(
    hosts_file: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    hosts_file.write_text("")
    monkeypatch.setattr(
        hostalias.subprocess, "run",
        lambda *a, **k: SimpleNamespace(returncode=1, stderr="nope"),
    )
    with pytest.raises(typer.BadParameter, match="sudo tee failed"):
        hostalias.install("new.host")
