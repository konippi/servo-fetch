# servo-fetch

[![CI](https://github.com/konippi/servo-fetch/actions/workflows/ci.yml/badge.svg)](https://github.com/konippi/servo-fetch/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/servo-fetch)](https://pypi.org/project/servo-fetch/)
[![Python](https://img.shields.io/pypi/pyversions/servo-fetch)](https://pypi.org/project/servo-fetch/)
[![uv](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/uv/main/assets/badge/v0.json)](https://github.com/astral-sh/uv)
[![Ruff](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/ruff/main/assets/badge/v2.json)](https://github.com/astral-sh/ruff)

Python bindings for [servo-fetch](https://github.com/konippi/servo-fetch) — fetch, render, and extract web content with an embedded Servo browser engine.

- **No Chromium** — single binary, no browser download
- **JavaScript execution** — full Servo engine with SpiderMonkey
- **Schema extraction** — declarative CSS-selector → structured JSON, no LLM
- **Async-ready** — `asyncio.to_thread` wrappers, `AsyncClient` with streaming crawl
- **Typed** — full `.pyi` stubs, works with ty / mypy / pyright

## Install

```bash
pip install servo-fetch
```

## Quick Start

```python
import servo_fetch
page = servo_fetch.fetch("https://example.com")
page.html          # rendered HTML
page.inner_text    # document.body.innerText
page.markdown      # readable Markdown (lazy, cached)
page.title         # str | None
```

## Schema Extraction

```python
from servo_fetch import Schema, Field

schema = Schema(
    base_selector=".product",
    fields=[
        Field(name="title", selector="h2", type="text"),
        Field(name="price", selector=".price", type="text"),
        Field(name="url", selector="a", type="attribute", attribute="href"),
    ],
)

page = servo_fetch.fetch("https://shop.example.com", schema=schema)
page.extracted  # [{"title": "...", "price": "...", "url": "..."}]
```

## Async

```python
from servo_fetch import fetch_async, AsyncClient

page = await fetch_async("https://example.com")

async with AsyncClient(user_agent="MyBot/1.0") as client:
    async for page in client.crawl_stream("https://docs.example.com", max_pages=50):
        print(page.url, page.title)
```

## Develop

Requires [uv](https://docs.astral.sh/uv/).

```bash
uv sync --group all              # create venv + install dev deps
uv run maturin develop           # build extension (debug, fast compile)
uv run pytest                    # run tests
uv run ruff check python tests   # lint
uv run ty check python           # type check
```

## Troubleshooting

### Linux: "cannot allocate memory in static TLS block"

Servo's native extension uses large thread-local storage. On some Linux systems, set before importing:

```bash
export GLIBC_TUNABLES=glibc.rtld.optional_static_tls=16384
```
