"""In-memory fixture HTTP server used by every benchmark."""
from __future__ import annotations

import contextlib
import gzip
import http.server
import socket
import threading
import time
from collections.abc import Iterator, Mapping
from pathlib import Path
from typing import Self


class _Handler(http.server.BaseHTTPRequestHandler):
    pages: Mapping[bytes, bytes]

    def do_GET(self) -> None:
        if self.path == "/":
            self._send(b"ok\n", "text/plain; charset=utf-8")
            return
        body = self.pages.get(self.path.encode())
        if body is None:
            self.send_error(404)
            return
        self._send(body, "text/html; charset=utf-8")

    def _send(self, body: bytes, ctype: str) -> None:
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args: object, **_kwargs: object) -> None:
        """Silence access-log spam."""


def _make_handler(pages: Mapping[bytes, bytes]) -> type[_Handler]:
    class BoundHandler(_Handler):
        pass

    BoundHandler.pages = pages
    return BoundHandler


class FixtureServer:
    """Serve preloaded `path → HTML bytes` on 127.0.0.1 as a context manager."""

    def __init__(self, pages: Mapping[str, bytes], port: int) -> None:
        self._pages: dict[bytes, bytes] = {
            (k if isinstance(k, bytes) else k.encode()): v for k, v in pages.items()
        }
        self._port = port
        self._server: http.server.ThreadingHTTPServer | None = None
        self._thread: threading.Thread | None = None

    def __enter__(self) -> Self:
        self._server = http.server.ThreadingHTTPServer(
            ("127.0.0.1", self._port), _make_handler(self._pages),
        )
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        self._wait_ready()
        return self

    def __exit__(self, *_exc: object) -> None:
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
        if self._thread is not None:
            self._thread.join(timeout=5)

    def _wait_ready(self, attempts: int = 50, delay: float = 0.1) -> None:
        for _ in range(attempts):
            with contextlib.suppress(OSError), socket.create_connection(
                ("127.0.0.1", self._port), timeout=0.2,
            ):
                return
            time.sleep(delay)
        raise RuntimeError(f"fixture server did not come up on :{self._port}")


@contextlib.contextmanager
def serve(pages: Mapping[str, bytes], port: int) -> Iterator[FixtureServer]:
    with FixtureServer(pages, port) as s:
        yield s


def load_local_fixtures(root: Path) -> dict[str, bytes]:
    pages: dict[str, bytes] = {
        f"/perf/{p.name}": p.read_bytes() for p in root.glob("perf/*.html")
    }
    pages.update(
        {f"/extraction/{p.name}": p.read_bytes() for p in root.glob("extraction/*.html")},
    )
    if not pages:
        raise FileNotFoundError(f"no HTML fixtures under {root}")
    return pages


def load_gzipped_html(html_dir: Path, limit: int = 0) -> tuple[dict[str, bytes], list[str]]:
    """Return (pages keyed by `/<id>.html`, ordered `<id>` list)."""
    files = sorted(html_dir.glob("*.html.gz"))
    if limit:
        files = files[:limit]
    pages: dict[str, bytes] = {}
    ids: list[str] = []
    for p in files:
        fid = p.stem.removesuffix(".html")
        pages[f"/{fid}.html"] = gzip.decompress(p.read_bytes())
        ids.append(fid)
    return pages, ids
