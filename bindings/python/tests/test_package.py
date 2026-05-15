"""Package-level surface: version, imports, docstrings."""

from __future__ import annotations

import re

import servo_fetch


def test_version_exposed() -> None:
    assert isinstance(servo_fetch.__version__, str)
    assert re.match(r"^\d+\.\d+\.\d+", servo_fetch.__version__)


def test_public_api() -> None:
    for name in (
        "fetch",
        "fetch_async",
        "Client",
        "AsyncClient",
        "Page",
        "Schema",
        "Field",
        "CrawlResult",
        "MappedUrl",
        "ConsoleMessage",
        "ServoFetchError",
        "InvalidUrlError",
        "FetchTimeoutError",
        "NetworkError",
        "EngineError",
        "SchemaError",
    ):
        assert hasattr(servo_fetch, name), name


def test_error_hierarchy() -> None:
    assert issubclass(servo_fetch.InvalidUrlError, servo_fetch.ServoFetchError)
    assert issubclass(servo_fetch.FetchTimeoutError, servo_fetch.ServoFetchError)
    assert issubclass(servo_fetch.NetworkError, servo_fetch.ServoFetchError)
    assert issubclass(servo_fetch.EngineError, servo_fetch.ServoFetchError)
    assert issubclass(servo_fetch.SchemaError, servo_fetch.ServoFetchError)
    assert issubclass(servo_fetch.ServoFetchError, Exception)


def test_docstrings_present() -> None:
    assert servo_fetch.Page.__doc__
    assert servo_fetch.Schema.__doc__
    assert servo_fetch.Field.__doc__
    assert servo_fetch.Client.__doc__
    assert servo_fetch.AsyncClient.__doc__
    assert servo_fetch.fetch.__doc__
    assert servo_fetch.fetch_async.__doc__
