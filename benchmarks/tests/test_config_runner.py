"""Config env-parsing + Runner behavior."""
from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest

from servo_fetch_bench import runners as runners_mod
from servo_fetch_bench.config import Config
from servo_fetch_bench.runners import Runner, _strip_inline_text, curl_baseline


def test_config_defaults(cfg: Config) -> None:
    assert (cfg.port, cfg.warmup, cfg.min_runs, cfg.memory_runs) == (8731, 3, 10, 5)
    assert cfg.parallel_urls == (1, 2, 4, 8)
    assert cfg.fixtures == ("static-small", "spa-light", "spa-heavy")
    assert cfg.bench_host == "bench.servo-fetch"
    assert cfg.node_bin == "node"


@pytest.mark.parametrize(
    ("env", "attr", "expected"),
    [
        ({"PORT": "9999"}, "port", 9999),
        ({"WARMUP": "0"}, "warmup", 0),
        ({"MIN_RUNS": "20"}, "min_runs", 20),
        ({"MEMORY_RUNS": "12"}, "memory_runs", 12),
        ({"FIXTURES": "a b c"}, "fixtures", ("a", "b", "c")),
        ({"PARALLEL_URLS": "3,7,11"}, "parallel_urls", (3, 7, 11)),
        ({"PARALLEL_URLS": ""}, "parallel_urls", (1, 2, 4, 8)),
        ({"BENCH_HOST": "example.test"}, "bench_host", "example.test"),
        ({"PORT": ""}, "port", 8731),
    ],
)
def test_config_env_override(
    clean_env: None,
    monkeypatch: pytest.MonkeyPatch,
    env: dict[str, str],
    attr: str,
    expected: object,
) -> None:
    for k, v in env.items():
        monkeypatch.setenv(k, v)
    assert getattr(Config(), attr) == expected


def test_config_is_frozen(cfg: Config) -> None:
    with pytest.raises(AttributeError):
        cfg.port = 0  # type: ignore[misc]


@pytest.mark.parametrize(
    ("name", "subpath", "expected_path"),
    [
        ("foo", "", "/foo.html"),
        ("foo", "extraction", "/extraction/foo.html"),
    ],
)
def test_config_fixture_url(
    cfg: Config, name: str, subpath: str, expected_path: str,
) -> None:
    expected = f"http://{cfg.bench_host}:{cfg.port}{expected_path}"
    assert cfg.fixture_url(name, subpath) == expected


@pytest.mark.parametrize(
    ("label", "expected"),
    [
        ("servo-fetch", "servo-fetch"),
        ("servo-fetch (layout-aware)", "servo-fetch-layout-aware"),
        ("foo:bar:", "foo-bar"),
        ("!!!", ""),
    ],
)
def test_runner_slug_is_filename_safe(label: str, expected: str) -> None:
    assert Runner(label, []).slug == expected


def test_runner_cmd_appends_urls_to_argv() -> None:
    assert Runner("x", ["bin", "--flag"]).cmd(["u1", "u2"]) == ["bin", "--flag", "u1", "u2"]


def test_runner_shell_quotes_paths_and_env() -> None:
    r = Runner("x", ["node", "/path to/runner.js"], env={"VAR": "v with spaces"})
    out = r.shell(["http://u"])
    assert out.startswith("env VAR='v with spaces' ")
    assert "'/path to/runner.js'" in out


def test_runner_shell_omits_env_prefix_when_env_empty() -> None:
    assert Runner("x", ["/bin/cmd", "--arg"]).shell(["http://u"]) == "/bin/cmd --arg http://u"


def test_runner_is_frozen() -> None:
    with pytest.raises(AttributeError):
        Runner("x", []).label = "y"  # type: ignore[misc]


def test_runner_run_returns_stdout() -> None:
    assert Runner("x", ["python3", "-c", "print('hello')"]).run([]).strip() == "hello"


def test_runner_run_propagates_env_to_subprocess() -> None:
    r = Runner(
        "x",
        ["python3", "-c", "import os; print(os.environ['MY_VAR'])"],
        env={"MY_VAR": "propagated"},
    )
    assert r.run([]).strip() == "propagated"


def test_runner_run_forwards_stderr_on_nonzero_exit(
    capfd: pytest.CaptureFixture[str],
) -> None:
    r = Runner("x", ["python3", "-c", "import sys; sys.stderr.write('boom'); sys.exit(1)"])
    assert r.run([]) == ""
    assert "boom" in capfd.readouterr().err


def test_runner_run_returns_empty_on_timeout(
    monkeypatch: pytest.MonkeyPatch,
    capfd: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.setattr(runners_mod, "RUN_TIMEOUT", 0.1)
    r = Runner("slow", ["python3", "-c", "import time; time.sleep(5)"])
    assert r.run([]) == ""
    assert "timed out" in capfd.readouterr().err


def test_runner_output_format_html_strips_tags() -> None:
    r = Runner("x", ["python3", "-c", "print('<p>hi</p>')"], output_format="html")
    assert r.run([]).strip() == "hi"


def test_runner_output_format_plain_preserves_stdout() -> None:
    assert Runner("x", ["python3", "-c", "print('<p>hi</p>')"]).run([]).strip() == "<p>hi</p>"


@pytest.mark.parametrize(
    ("urls", "expected_invocations"),
    [(["u1"], 1), (["u1", "u2", "u3"], 3)],
    ids=["one-url", "three-urls"],
)
def test_runner_single_url_mode_invokes_per_url(
    urls: list[str], expected_invocations: int,
) -> None:
    r = Runner("x", ["python3", "-c", "print('INVOKE')"], single_url=True)
    assert r.run(urls).count("INVOKE") == expected_invocations


def test_runner_multi_url_mode_invokes_once_regardless_of_url_count() -> None:
    r = Runner("x", ["python3", "-c", "print('INVOKE')"])
    assert r.run(["u1", "u2", "u3"]).count("INVOKE") == 1


def test_strip_inline_text_removes_scripts_styles_and_tags() -> None:
    html = (
        "<html><head><style>x{}</style></head>"
        "<body><script>alert(1)</script>\n<h1>hi</h1>\n<p>world</p></body></html>"
    )
    assert _strip_inline_text(html) == "hi\nworld"


def test_curl_baseline_returns_empty_when_curl_missing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def boom(*_a: object, **_k: object) -> None:
        raise FileNotFoundError

    monkeypatch.setattr(runners_mod.subprocess, "check_output", boom)
    assert curl_baseline(["http://example.test"]).strip() == ""


def test_speed_runners_always_include_servo_fetch_and_playwright(cfg: Config) -> None:
    labels = [r.label for r in runners_mod.speed_runners(cfg)]
    assert "servo-fetch" in labels
    assert "playwright:optimized" in labels


@pytest.mark.parametrize(
    ("has_chs", "has_lp", "expected_optionals"),
    [
        (True, False, {"chrome-headless-shell"}),
        (False, True, {"lightpanda"}),
        (True, True, {"chrome-headless-shell", "lightpanda"}),
        (False, False, set()),
    ],
    ids=["chs-only", "lp-only", "both", "neither"],
)
def test_speed_runners_include_optional_peers_when_available(
    cfg: Config,
    monkeypatch: pytest.MonkeyPatch,
    has_chs: bool,
    has_lp: bool,
    expected_optionals: set[str],
) -> None:
    monkeypatch.setattr(runners_mod, "_has_chrome_headless_shell", lambda _c: has_chs)
    monkeypatch.setattr(runners_mod, "_has_lightpanda", lambda _c: has_lp)
    labels = {r.label for r in runners_mod.speed_runners(cfg)}
    assert labels == {"servo-fetch", "playwright:optimized"} | expected_optionals


@pytest.mark.parametrize(
    ("engine_attr", "has_fn_name"),
    [
        ("chrome_headless_shell_bin", "_has_chrome_headless_shell"),
        ("lightpanda_bin", "_has_lightpanda"),
    ],
    ids=["chrome-headless-shell", "lightpanda"],
)
@pytest.mark.parametrize(
    ("state", "expected"),
    [("missing", False), ("non-executable", False), ("executable", True)],
)
def test_peer_engine_detection_requires_executable_file(
    cfg: Config,
    make_stub_bin: Callable[..., Path],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    engine_attr: str,
    has_fn_name: str,
    state: str,
    expected: bool,
) -> None:
    if state == "missing":
        stub_path = tmp_path / "ghost"
    else:
        stub_path = make_stub_bin("stub", executable=(state == "executable"))
    monkeypatch.setattr(Config, engine_attr, stub_path)
    has_fn = getattr(runners_mod, has_fn_name)
    assert has_fn(cfg) is expected


@pytest.fixture
def both_peers_stubbed(
    cfg: Config,
    make_stub_bin: Callable[..., Path],
    monkeypatch: pytest.MonkeyPatch,
) -> dict[str, Path]:
    """Install executable stubs for both optional peer engines."""
    chs_bin = make_stub_bin("chrome-headless-shell")
    lp_bin = make_stub_bin("lightpanda")
    monkeypatch.setattr(Config, "chrome_headless_shell_bin", chs_bin)
    monkeypatch.setattr(Config, "lightpanda_bin", lp_bin)
    return {"chrome-headless-shell": chs_bin, "lightpanda": lp_bin}


def test_peer_runners_invoke_engine_binary_directly_not_via_node(
    cfg: Config, both_peers_stubbed: dict[str, Path],
) -> None:
    # Regression: the fairness-axis peers must not route through Node.js.
    # The first argv element has to be the engine itself, not a wrapper.
    by_label = {r.label: r for r in runners_mod.speed_runners(cfg)}
    assert by_label["chrome-headless-shell"].argv_prefix[0] == str(
        both_peers_stubbed["chrome-headless-shell"],
    )
    assert by_label["lightpanda"].argv_prefix[0] == str(both_peers_stubbed["lightpanda"])
    # playwright:optimized IS Node-wrapped by design, as its axis dictates.
    assert by_label["playwright:optimized"].argv_prefix[0] == cfg.node_bin


@pytest.mark.parametrize("engine", ["chrome-headless-shell", "lightpanda"])
def test_peer_runners_use_html_output_format_and_single_url_mode(
    cfg: Config, both_peers_stubbed: dict[str, Path], engine: str,
) -> None:
    # Both DOM-dumping engines must share the Python tag-strip pass, and
    # both require one URL per invocation (e.g. --dump-dom).
    by_label = {r.label: r for r in runners_mod.speed_runners(cfg)}
    assert by_label[engine].output_format == "html"
    assert by_label[engine].single_url is True


def test_extract_runners_contain_only_servo_fetch_and_readability(cfg: Config) -> None:
    labels = [r.label for r in runners_mod.extract_runners(cfg)]
    assert labels == ["servo-fetch (layout-aware)", "Readability (DOM-only)"]
