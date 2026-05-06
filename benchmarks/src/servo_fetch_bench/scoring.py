"""Word-level F1 scoring for extraction benchmarks."""
from __future__ import annotations

import re
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

_WORD = re.compile(r"\w+", re.UNICODE)
_WS = re.compile(r"\s+")


@dataclass(frozen=True, slots=True)
class Score:
    f1: float
    precision: float
    recall: float


@dataclass(frozen=True, slots=True)
class Snippets:
    with_coverage: float
    without_coverage: float
    missing: tuple[str, ...]
    leaked: tuple[str, ...]


def word_f1(predicted: str, reference: str) -> Score:
    """Multiset word overlap F1 between `predicted` and `reference` (case-insensitive)."""
    pred = Counter(_WORD.findall(predicted.lower()))
    ref = Counter(_WORD.findall(reference.lower()))
    if not ref:
        # Conventional: both empty → perfect; non-empty pred with empty ref → all false-positive.
        return Score(1.0, 1.0, 1.0) if not pred else Score(0.0, 0.0, 1.0)
    if not pred:
        return Score(0.0, 1.0, 0.0)
    overlap = sum((pred & ref).values())
    p = overlap / pred.total()
    r = overlap / ref.total()
    f1 = 2 * p * r / (p + r) if p + r else 0.0
    return Score(f1, p, r)


def snippet_coverage(predicted: str, withs: list[str], withouts: list[str]) -> Snippets:
    """Fraction of required snippets present / forbidden snippets absent."""
    norm = _normalize(predicted)
    withs = [s for s in withs if _normalize(s)]
    withouts = [s for s in withouts if _normalize(s)]
    missing = tuple(s for s in withs if _normalize(s) not in norm)
    leaked = tuple(s for s in withouts if _normalize(s) in norm)
    return Snippets(
        with_coverage=1.0 if not withs else (len(withs) - len(missing)) / len(withs),
        without_coverage=1.0 if not withouts else (len(withouts) - len(leaked)) / len(withouts),
        missing=missing,
        leaked=leaked,
    )


def parse_expect(path: Path) -> tuple[list[str], list[str]]:
    """Parse fixture expectation files."""
    withs: list[str] = []
    withouts: list[str] = []
    buckets = {"with": withs, "without": withouts}
    uses_kv: bool | None = None
    for lineno, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        s = raw.strip()
        if not s or s.startswith("#"):
            continue
        if uses_kv is None:
            uses_kv = bool(re.match(r"(?i)(with|without)\s*=", s))
        if not uses_kv:
            withs.append(s)
            continue
        key, sep, value = s.partition("=")
        if not sep:
            raise ValueError(f"{path}:{lineno}: expected 'key = value'")
        bucket = buckets.get(key.strip().lower())
        if bucket is None:
            raise ValueError(f"{path}:{lineno}: unknown key {key.strip()!r}")
        bucket.append(value.strip())
    return withs, withouts


def _normalize(text: str) -> str:
    return _WS.sub(" ", text).strip().lower()
