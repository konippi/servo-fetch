"""Edge cases for word-level F1 scoring."""
from __future__ import annotations

from pathlib import Path

import pytest

from servo_fetch_bench.scoring import (
    Score,
    Snippets,
    parse_expect,
    snippet_coverage,
    word_f1,
)


@pytest.mark.parametrize(
    ("pred", "ref", "expected"),
    [
        ("hello world", "hello world", Score(1.0, 1.0, 1.0)),
        ("", "", Score(1.0, 1.0, 1.0)),
        ("anything here", "", Score(0.0, 0.0, 1.0)),
        ("alpha beta", "gamma delta", Score(0.0, 0.0, 0.0)),
    ],
)
def test_word_f1_exact(pred: str, ref: str, expected: Score) -> None:
    assert word_f1(pred, ref) == expected


@pytest.mark.parametrize(
    ("pred", "ref", "precision", "recall", "f1"),
    [
        ("hello world foo", "hello world", 2 / 3, 1.0, 0.8),
        ("", "hello", 1.0, 0.0, 0.0),
        ("hello hello", "hello", 0.5, 1.0, 2 / 3),
        ("   \n\t", "hello", 1.0, 0.0, 0.0),
    ],
)
def test_word_f1_approx(
    pred: str, ref: str, precision: float, recall: float, f1: float,
) -> None:
    s = word_f1(pred, ref)
    assert s.precision == pytest.approx(precision)
    assert s.recall == pytest.approx(recall)
    assert s.f1 == pytest.approx(f1)


@pytest.mark.parametrize(
    ("pred", "ref"),
    [
        ("HELLO world", "hello WORLD"),
        ("見出し 本文", "見出し 本文"),
        ("café 日本語 naïve", "café 日本語 naïve"),
        ("hello, world!", "hello world"),
    ],
    ids=["case", "japanese", "mixed-scripts", "punctuation"],
)
def test_word_f1_normalized_match_is_perfect(pred: str, ref: str) -> None:
    assert word_f1(pred, ref).f1 == pytest.approx(1.0)


def test_snippet_coverage_all_pass() -> None:
    r = snippet_coverage("nav intro body footer", ["intro", "body"], ["ads", "tracking"])
    assert r == Snippets(1.0, 1.0, (), ())


def test_snippet_coverage_reports_missing_with_snippets() -> None:
    r = snippet_coverage("body only", ["intro", "body"], [])
    assert r.with_coverage == 0.5
    assert r.missing == ("intro",)


def test_snippet_coverage_reports_leaked_without_snippets() -> None:
    r = snippet_coverage("body ads", [], ["ads"])
    assert r.without_coverage == 0.0
    assert r.leaked == ("ads",)


@pytest.mark.parametrize(
    ("pred", "needle"),
    [
        ("hello\n\n  world", "hello world"),
        ("HELLO WORLD", "hello world"),
    ],
    ids=["whitespace-normalized", "case-insensitive"],
)
def test_snippet_coverage_needle_matches_after_normalization(
    pred: str, needle: str,
) -> None:
    assert snippet_coverage(pred, [needle], []).with_coverage == 1.0


def test_snippet_coverage_empty_lists_are_vacuous() -> None:
    assert snippet_coverage("anything", [], []) == Snippets(1.0, 1.0, (), ())


def test_snippets_is_frozen() -> None:
    with pytest.raises(AttributeError):
        snippet_coverage("x", [], []).with_coverage = 0.0  # type: ignore[misc]


def test_parse_expect_kv_flavor(tmp_path: Path) -> None:
    p = tmp_path / "x.expect"
    p.write_text("# comment\nwith = alpha\nwithout = beta\n\nwith = gamma\n")
    assert parse_expect(p) == (["alpha", "gamma"], ["beta"])


def test_parse_expect_plain_flavor(tmp_path: Path) -> None:
    p = tmp_path / "plain.expect"
    p.write_text("# header\nfirst line\nsecond <b>line</b>\n")
    assert parse_expect(p) == (["first line", "second <b>line</b>"], [])


def test_parse_expect_kv_keys_are_case_insensitive(tmp_path: Path) -> None:
    p = tmp_path / "x.expect"
    p.write_text("WITH = up\nWithout = down\n")
    assert parse_expect(p) == (["up"], ["down"])


@pytest.mark.parametrize(
    ("contents", "match"),
    [
        ("with = ok\nbogus = no\n", "unknown key"),
        ("with = ok\nbroken line\n", "expected 'key = value'"),
    ],
    ids=["unknown-key", "missing-equals"],
)
def test_parse_expect_kv_rejects_invalid(
    tmp_path: Path, contents: str, match: str,
) -> None:
    p = tmp_path / "x.expect"
    p.write_text(contents)
    with pytest.raises(ValueError, match=match):
        parse_expect(p)


@pytest.mark.parametrize(
    "contents", ["", "# just\n# comments\n\n"],
    ids=["empty", "comments"],
)
def test_parse_expect_empty_inputs_yield_empty_lists(
    tmp_path: Path, contents: str,
) -> None:
    p = tmp_path / "x.expect"
    p.write_text(contents)
    assert parse_expect(p) == ([], [])
