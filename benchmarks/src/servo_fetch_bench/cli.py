"""servo-fetch benchmark CLI."""
from __future__ import annotations

import json
import os
import statistics
import subprocess
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console
from rich.progress import track

from . import binaries, datasets, hostalias, hyperfine, measure, reporting, scoring, sizes
from .config import Config
from .fixtures import load_gzipped_html, load_local_fixtures, serve
from .runners import (
    EXTRACTION_FIXTURES,
    Runner,
    curl_baseline,
    extract_runners,
    speed_runners,
)

app = typer.Typer(
    add_completion=False,
    no_args_is_help=True,
    help="servo-fetch benchmark harness.",
)
extract_app = typer.Typer(no_args_is_help=True, help="Extraction-quality benchmarks.")
app.add_typer(extract_app, name="extract")

console = Console(stderr=True)


@app.callback()
def _main(ctx: typer.Context) -> None:
    ctx.obj = Config()


def _cfg(ctx: typer.Context) -> Config:
    return ctx.obj  # type: ignore[no-any-return]


def _export_node_path(cfg: Config) -> None:
    """Set NODE_PATH for child processes spawned by hyperfine's /usr/bin/env."""
    os.environ["NODE_PATH"] = cfg.node_path()


def _prepare_cmd(ctx: typer.Context, *, require_host: bool = True) -> Config:
    """Standard command startup: resolve config, check host alias, create results dir."""
    cfg = _cfg(ctx)
    if require_host:
        hostalias.require(cfg.bench_host)
    cfg.results_dir.mkdir(parents=True, exist_ok=True)
    return cfg


def _write_report(path: Path, cfg: Config, title: str, body: str) -> None:
    """Write a Markdown report with standard env header + announce on console."""
    path.write_text(
        f"{reporting.env_header(cfg)}# {title}\n\n{body}",
        encoding="utf-8",
    )
    console.print(f"\nwrote [cyan]{path}[/]")


def _skip_or_fail(r: Runner, reason: str) -> None:
    """Raise BadParameter if the runner is required; else console-warn and return."""
    msg = f"runner probe failed: {r.label} ({reason})"
    if r.required:
        raise typer.BadParameter(msg)
    console.print(f"[yellow]skip[/] {msg}")


@app.command()
def setup(ctx: typer.Context) -> None:
    """Install /etc/hosts alias (sudo)."""
    hostalias.install(_cfg(ctx).bench_host)


@app.command("install-binaries")
def install_binaries(
    ctx: typer.Context,
    which: Annotated[
        binaries.InstallTarget,
        typer.Argument(help="Which binary to install."),
    ] = "all",
) -> None:
    """Download optional peer engines (chrome-headless-shell, lightpanda)."""
    binaries.install(_cfg(ctx).bin_dir, which)


@app.command()
def download(dataset: datasets.Dataset = datasets.Dataset.SCRAPINGHUB) -> None:
    """Fetch an extraction benchmark dataset into benchmarks/data/."""
    datasets.download(dataset)


@app.command()
def equivalence(ctx: typer.Context) -> None:
    """Verify every runner extracts the required substrings per fixture."""
    cfg = _prepare_cmd(ctx)

    runners_ = speed_runners(cfg)
    rows: list[list[str]] = []
    failures = tolerated = 0

    with serve(load_local_fixtures(cfg.fixtures_dir), cfg.port):
        for fixture in cfg.fixtures:
            console.rule(f"[bold]{fixture}")
            withs, _ = scoring.parse_expect(cfg.fixtures_dir / "perf" / "golden" / f"{fixture}.expect")
            url = cfg.fixture_url(fixture, "perf")
            checks: list[tuple[str, str, bool]] = [
                ("curl", curl_baseline([url]), False),
                *((r.label, r.run([url]), r.required) for r in runners_),
            ]
            for label, output, required in checks:
                missing = list(scoring.snippet_coverage(output, withs, []).missing)
                status, marker = _eq_status(missing, required)
                rows.append([f"`{label}`", f"`{fixture}`", status, ", ".join(missing)])
                console.print(f"  [{marker}]{label:<22}[/]  {fixture}"
                              + (f"  missing: {missing}" if missing else ""))
                if missing and required:
                    failures += 1
                elif missing:
                    tolerated += 1

    _write_report(
        cfg.results_dir / "equivalence.md", cfg, "Equivalence check",
        reporting.md_table(["Tool", "Fixture", "Status", "Missing"], rows),
    )
    if failures:
        console.print(f"[red]FAIL[/] ({failures} required, {tolerated} tolerated)")
        raise typer.Exit(1)
    console.print(f"[green]pass[/] ({tolerated} tolerated)")


def _eq_status(missing: list[str], required: bool) -> tuple[str, str]:
    if not missing:
        return "✅ pass", "green"
    return ("❌ **FAIL**", "red") if required else ("⚠️ expected-fail", "yellow")


def _probe_working_runners(cfg: Config) -> list[Runner]:
    """Drop runners whose first invocation fails; required ones raise."""
    probe_url = cfg.fixture_url(cfg.fixtures[0], "perf")
    kept: list[Runner] = []
    for r in speed_runners(cfg):
        try:
            proc = subprocess.run(
                [*r.argv_prefix, probe_url],
                capture_output=True, text=True, check=False,
                env={**os.environ, **r.env},
                timeout=30,
            )
        except (subprocess.TimeoutExpired, FileNotFoundError) as e:
            _skip_or_fail(r, type(e).__name__)
            continue
        if proc.returncode == 0:
            kept.append(r)
        else:
            _skip_or_fail(r, f"exit {proc.returncode}")
    return kept


@app.command()
def time(ctx: typer.Context) -> None:
    """Wall-clock time per (runner × fixture) via hyperfine."""
    cfg = _prepare_cmd(ctx)
    hyperfine.require()
    _export_node_path(cfg)

    combined = cfg.results_dir / "time.md"
    combined.write_text(reporting.env_header(cfg) + "# Time benchmarks\n\n", encoding="utf-8")

    with serve(load_local_fixtures(cfg.fixtures_dir), cfg.port):
        runners_ = _probe_working_runners(cfg)
        for fixture in cfg.fixtures:
            url = cfg.fixture_url(fixture, "perf")
            console.rule(f"[bold]{fixture}[/]  →  {url}")
            md = cfg.results_dir / f"time-{fixture}.md"
            hyperfine.run(
                [(r.label, r.shell([url])) for r in runners_],
                warmup=cfg.warmup, min_runs=cfg.min_runs,
                export_md=md, export_json=cfg.results_dir / f"time-{fixture}.json",
            )
            with combined.open("a", encoding="utf-8") as f:
                f.write(f"## {fixture}\n\n{md.read_text(encoding='utf-8')}\n")
    console.print(f"\nwrote [cyan]{combined}[/]")


@app.command()
def parallel(
    ctx: typer.Context,
    fixture: Annotated[str, typer.Option(help="Fixture to parallel-fetch.")] = "spa-light",
) -> None:
    """Scalability curve: time vs URL count."""
    cfg = _prepare_cmd(ctx)
    hyperfine.require()
    _export_node_path(cfg)

    out = cfg.results_dir / "parallel.md"
    out.write_text(
        reporting.env_header(cfg) + f"# Parallel scalability\n\nFixture: `{fixture}`.\n\n",
        encoding="utf-8",
    )
    # Parallel uses Markdown pipeline (servo-fetch --raw text forbids multi-URL).
    # Only multi-URL-capable runners participate.
    runners_ = [
        Runner("servo-fetch", [str(cfg.servo_fetch_bin), "-q"]),
        *(r for r in speed_runners(cfg) if r.label == "playwright:optimized"),
    ]
    with serve(load_local_fixtures(cfg.fixtures_dir), cfg.port):
        url = cfg.fixture_url(fixture, "perf")
        for n in cfg.parallel_urls:
            console.rule(f"[bold]N = {n}")
            md = cfg.results_dir / f"parallel-n{n}.md"
            hyperfine.run(
                [(f"{r.label} (N={n})", r.shell([url] * n)) for r in runners_],
                warmup=cfg.warmup, min_runs=cfg.min_runs,
                export_md=md, export_json=cfg.results_dir / f"parallel-n{n}.json",
            )
            with out.open("a", encoding="utf-8") as f:
                f.write(f"## N = {n}\n\n{md.read_text(encoding='utf-8')}\n")
    console.print(f"\nwrote [cyan]{out}[/]")


@app.command()
def memory(
    ctx: typer.Context,
    cgmemtime: Annotated[
        bool, typer.Option("--cgmemtime", help="Use cgmemtime on Linux (most precise)."),
    ] = False,
    tree: Annotated[
        bool,
        typer.Option(
            "--tree/--no-tree",
            help="Sample the full process tree (default). --no-tree reverts to "
                 "legacy /usr/bin/time behavior that cannot see Chromium helpers.",
        ),
    ] = True,
) -> None:
    """Peak resident memory per (runner × fixture)."""
    cfg = _prepare_cmd(ctx)

    runners_ = [Runner("curl", ["curl", "-sf"]), *speed_runners(cfg)]
    rows: list[list[str]] = []

    with serve(load_local_fixtures(cfg.fixtures_dir), cfg.port):
        # Drop runners that crash on probe (see bench time).
        probed_labels = {r.label for r in _probe_working_runners(cfg)}
        runners_ = [r for r in runners_ if r.label == "curl" or r.label in probed_labels]
        for fixture in cfg.fixtures:
            console.rule(f"[bold]{fixture}")
            url = cfg.fixture_url(fixture, "perf")
            for r in runners_:
                samples = [
                    measure.peak_bytes(
                        [*r.argv_prefix, url], env=r.env or None,
                        use_cgmemtime=cgmemtime, tree=tree,
                    )
                    for _ in range(cfg.memory_runs)
                ]
                lo, mid, hi = min(samples), int(statistics.median(samples)), max(samples)
                console.print(
                    f"  {r.label:<22} min={measure.human(lo):<10} "
                    f"median={measure.human(mid):<10} max={measure.human(hi)}",
                )
                rows.append([
                    f"{r.label} / {fixture}",
                    measure.human(lo), measure.human(mid), measure.human(hi),
                ])

    mode = (
        "cgmemtime (whole cgroup, Linux)" if cgmemtime
        else "psutil tree polling (parent + all descendants, 10ms cadence)"
        if tree
        else "/usr/bin/time (main process only — legacy, understates Chromium)"
    )
    _write_report(
        cfg.results_dir / "memory.md", cfg, "Memory benchmarks",
        f"Median of {cfg.memory_runs} runs. Measurement: {mode}.\n\n"
        + reporting.md_table(["Tool × Fixture", "min", "median", "max"], rows),
    )


@app.command()
def size(ctx: typer.Context) -> None:
    """Binary footprint per peer engine (size + non-system deps)."""
    cfg = _prepare_cmd(ctx, require_host=False)

    entries = []
    for e in sizes.measure(cfg):
        console.print(
            f"  {e.label:<24}"
            + (f" size={e.size_mib:6.1f} MiB"
               if e.size_mib is not None else "  (not found)              ")
            + (f"  deps={e.non_system_deps}" if e.non_system_deps is not None else "")
            + (f"  path={e.binary}" if e.binary else ""),
        )
        entries.append(e)

    _write_report(
        cfg.results_dir / "size.md", cfg,
        "Binary footprint",
        _size_table(entries),
    )


def _size_table(entries: list[sizes.SizeEntry]) -> str:
    rows = [
        [
            f"`{e.label}`",
            f"{e.size_mib:.1f} MiB" if e.size_mib is not None else "—",
            str(e.non_system_deps) if e.non_system_deps is not None else "—",
            str(e.binary) if e.binary else "—",
        ]
        for e in entries
    ]
    return (
        reporting.md_table(
            ["Tool", "Binary size", "Non-system deps¹", "Path"],
            rows, ["-", "-:", "-:", "-"],
        )
        + "\n¹ Dynamic libraries outside `/System/Library`, `/usr/lib`, `/lib`, `/lib64`. "
        "System stdlib doesn't count against a zero-deps claim — it's on every "
        "installed OS already.\n"
    )


@extract_app.command("layout")
def extract_layout(ctx: typer.Context) -> None:
    """Extraction quality on page-type fixtures (F1 + with/without)."""
    cfg = _prepare_cmd(ctx)

    lc = cfg.fixtures_dir / "extraction"
    runners_ = extract_runners(cfg)
    totals: dict[str, list[tuple[float, float, float]]] = {r.label: [] for r in runners_}
    sections: list[str] = []

    with serve(load_local_fixtures(cfg.fixtures_dir), cfg.port):
        for fx in EXTRACTION_FIXTURES:
            console.rule(f"[bold]{fx}")
            url = cfg.fixture_url(fx, "extraction")
            golden = (lc / "golden" / f"{fx}.txt").read_text(encoding="utf-8")
            withs, withouts = scoring.parse_expect(lc / "golden" / f"{fx}.expect")
            rows: list[list[str]] = []
            for r in runners_:
                pred = r.run([url])
                s = scoring.word_f1(pred, golden)
                sn = scoring.snippet_coverage(pred, withs, withouts)
                console.print(
                    f"  {r.label:<32} F1={s.f1:.4f} "
                    f"with={sn.with_coverage * 100:3.0f}% "
                    f"without={sn.without_coverage * 100:3.0f}%",
                )
                rows.append([
                    r.label, f"{s.f1:.3f}",
                    f"{sn.with_coverage * 100:.0f}%",
                    f"{sn.without_coverage * 100:.0f}%",
                ])
                totals[r.label].append((s.f1, sn.with_coverage, sn.without_coverage))
            sections.append(
                f"## `{fx}`\n\n"
                + reporting.md_table(
                    ["Extractor", "F1", "with[]", "without[]"], rows, ["-", "-:", "-:", "-:"],
                ),
            )

    summary = [
        [
            label,
            f"{sum(x[0] for x in v) / len(v):.3f}",
            f"{sum(x[1] for x in v) / len(v) * 100:.1f}%",
            f"{sum(x[2] for x in v) / len(v) * 100:.1f}%",
        ]
        for label, v in totals.items()
    ]
    _write_report(
        cfg.results_dir / "extraction-layout.md", cfg,
        "Extraction quality — page-type fixtures",
        "\n".join(sections)
        + f"\n## Summary — mean across {len(EXTRACTION_FIXTURES)} fixtures\n\n"
        + reporting.md_table(
            ["Extractor", "Mean F1", "Mean with[]", "Mean without[]"],
            summary, ["-", "-:", "-:", "-:"],
        ),
    )


@extract_app.command("dataset")
def extract_dataset(
    ctx: typer.Context,
    dataset: datasets.Dataset = datasets.Dataset.SCRAPINGHUB,
    limit: Annotated[int, typer.Option(help="Cap to first N pages (0 = all).")] = 0,
) -> None:
    """Extraction quality on scrapinghub/article-extraction-benchmark (MIT, 2020)."""
    cfg = _prepare_cmd(ctx)
    if not datasets.is_available(dataset):
        raise typer.BadParameter(
            f"{dataset.value} not downloaded. run:  ./benchmarks/bench download",
        )

    pages, ground, order = _prepare_dataset(dataset, limit)
    runners_ = extract_runners(cfg)
    pred_dir = cfg.results_dir / f"{dataset.value}-predictions"
    pred_dir.mkdir(exist_ok=True)
    ds_port = cfg.port + 1

    sections: list[str] = []
    with serve(pages, ds_port):
        for r in runners_:
            console.rule(f"[bold]{r.label}")
            preds: dict[str, dict[str, str]] = {}
            for fid in track(order, description=r.label, console=console, transient=True):
                preds[fid] = {
                    "articleBody": r.run([f"http://{cfg.bench_host}:{ds_port}/{fid}.html"]),
                }
            (pred_dir / f"{r.slug}.json").write_text(
                json.dumps({"output": preds}, ensure_ascii=False),
                encoding="utf-8",
            )
            sections += [f"## {r.label}\n", _dataset_table(ground, preds), ""]

    _write_report(
        cfg.results_dir / f"extraction-{dataset.value}.md", cfg,
        f"Extraction quality — {dataset.value} (MIT, 2020)",
        "\n".join(sections),
    )


def _prepare_dataset(
    ds: datasets.Dataset, limit: int,
) -> tuple[dict[str, bytes], dict[str, dict], list[str]]:
    base = datasets.path_for(ds)
    pages, ids = load_gzipped_html(base / "html", limit=limit)
    ground: dict[str, dict] = json.loads(
        (base / "ground-truth.json").read_text(encoding="utf-8"),
    )
    return pages, ground, ids


def _dataset_table(ground: dict[str, dict], preds: dict) -> str:
    scores: list[scoring.Score] = []
    for fid, item in ground.items():
        ref = item.get("articleBody") or ""
        pred = preds.get(fid, {}).get("articleBody") or ""
        scores.append(scoring.word_f1(pred, ref))

    n = len(scores) or 1
    rows = [[
        "**overall**", str(len(scores)),
        f"{sum(s.f1 for s in scores) / n:.3f}",
        f"{sum(s.precision for s in scores) / n:.3f}",
        f"{sum(s.recall for s in scores) / n:.3f}",
    ]]
    return reporting.md_table(
        ["Split", "N", "F1", "Precision", "Recall"],
        rows, ["-", "-:", "-:", "-:", "-:"],
    )


@app.command(name="all")
def all_(ctx: typer.Context) -> None:
    """Run every benchmark that does not require external datasets."""
    for cmd in (size, equivalence, time, memory, parallel, extract_layout):
        try:
            ctx.invoke(cmd, ctx=ctx)
        except typer.Exit as e:
            # Continue on clean exit (code 0, e.g. equivalence reporting tolerated fails);
            # abort on real failure (non-zero exit_code).
            if e.exit_code:
                raise

