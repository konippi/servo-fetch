"""Dataset download helper for extraction benchmarks."""
from __future__ import annotations

import io
import os
import tarfile
import urllib.request
from enum import StrEnum
from pathlib import Path

from rich.console import Console

from .config import BENCH_DIR

DATA_DIR = BENCH_DIR / "data"


class Dataset(StrEnum):
    """Extraction benchmark datasets."""

    SCRAPINGHUB = "scrapinghub"

_SCRAPINGHUB_TAG = os.environ.get("SCRAPINGHUB_REVISION", "v1.0.0")
_SCRAPINGHUB_URL = (
    "https://github.com/scrapinghub/article-extraction-benchmark/"
    f"archive/refs/tags/{_SCRAPINGHUB_TAG}.tar.gz"
)

_console = Console(stderr=True)


def path_for(ds: Dataset) -> Path:
    return DATA_DIR / ds.value


def is_available(ds: Dataset) -> bool:
    return (path_for(ds) / "ground-truth.json").is_file()


def download(ds: Dataset) -> None:
    match ds:
        case Dataset.SCRAPINGHUB:
            _download_scrapinghub()


def _download_scrapinghub() -> None:
    out = path_for(Dataset.SCRAPINGHUB)
    if (out / "ground-truth.json").is_file():
        _console.print(f"scrapinghub: already present at [cyan]{out}[/]")
        return
    out.mkdir(parents=True, exist_ok=True)
    _console.print(f"scrapinghub: downloading [cyan]{_SCRAPINGHUB_URL}[/]")
    with urllib.request.urlopen(_SCRAPINGHUB_URL, timeout=120) as resp:
        buf = io.BytesIO(resp.read())
    out_resolved = out.resolve()
    with tarfile.open(fileobj=buf, mode="r:gz") as tar:
        for member in tar.getmembers():
            parts = Path(member.name).parts
            if len(parts) < 2:
                continue
            rel = Path(*parts[1:])
            if rel.parts[0] not in {"html", "ground-truth.json"}:
                continue
            # Path-traversal guard: ensure extraction stays under `out`.
            if not (out / rel).resolve().is_relative_to(out_resolved):
                msg = f"refusing unsafe tar entry: {member.name}"
                raise ValueError(msg)
            member.name = str(rel)
            # Path traversal guarded above; safe to extract.
            tar.extract(member, out)
    _console.print(f"scrapinghub: extracted to [cyan]{out}[/]")
