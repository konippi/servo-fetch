import os
import threading
from collections.abc import Callable
from typing import Any, Literal

__version__: str

ConsoleLevel = Literal["log", "info", "warn", "error", "debug"]
FieldType = Literal["text", "attribute", "html", "inner_html", "nested_list"]

class ConsoleMessage:
    @property
    def level(self) -> ConsoleLevel: ...
    @property
    def message(self) -> str: ...

class Field:
    def __init__(
        self,
        *,
        name: str,
        selector: str,
        type: FieldType,
        attribute: str | None = None,
        fields: list[Field] | None = None,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def selector(self) -> str: ...
    @property
    def type(self) -> FieldType: ...

class Schema:
    def __init__(
        self,
        *,
        base_selector: str | None = None,
        fields: list[Field] = ...,
    ) -> None: ...
    @staticmethod
    def from_dict(data: dict[str, Any]) -> Schema: ...
    @staticmethod
    def from_json(json_str: str) -> Schema: ...
    @staticmethod
    def from_file(path: str | os.PathLike[str]) -> Schema: ...
    def extract(self, html: str) -> dict[str, Any] | list[dict[str, Any]]: ...

class Page:
    @property
    def url(self) -> str: ...
    @property
    def html(self) -> str: ...
    @property
    def inner_text(self) -> str: ...
    @property
    def title(self) -> str | None: ...
    @property
    def markdown(self) -> str: ...
    @property
    def extracted(self) -> dict[str, Any] | list[dict[str, Any]] | None: ...
    @property
    def screenshot(self) -> bytes | None: ...
    @property
    def js_result(self) -> str | None: ...
    @property
    def console(self) -> list[ConsoleMessage]: ...
    def save_screenshot(self, path: str | os.PathLike[str]) -> None: ...
    def to_json(self) -> str: ...

class CrawlResult:
    @property
    def url(self) -> str: ...
    @property
    def depth(self) -> int: ...
    @property
    def title(self) -> str | None: ...
    @property
    def content(self) -> str | None: ...
    @property
    def links_found(self) -> int | None: ...
    @property
    def error(self) -> str | None: ...
    @property
    def ok(self) -> bool: ...

class MappedUrl:
    @property
    def url(self) -> str: ...
    @property
    def lastmod(self) -> str | None: ...

class Client:
    def __init__(
        self,
        *,
        timeout: float = 30.0,
        settle: float = 0.0,
        user_agent: str | None = None,
    ) -> None: ...
    def fetch(
        self,
        url: str,
        *,
        timeout: float | None = None,
        settle: float | None = None,
        screenshot: bool = False,
        javascript: str | None = None,
        schema: Schema | None = None,
    ) -> Page: ...
    def crawl(
        self,
        url: str,
        *,
        max_pages: int = 50,
        max_depth: int = 3,
        include: str | list[str] | None = None,
        exclude: str | list[str] | None = None,
        concurrency: int = 1,
        delay_ms: int = 0,
    ) -> list[CrawlResult]: ...
    def crawl_each(
        self,
        url: str,
        callback: Callable[[CrawlResult], None],
        *,
        max_pages: int = 50,
        max_depth: int = 3,
        include: str | list[str] | None = None,
        exclude: str | list[str] | None = None,
        concurrency: int = 1,
        delay_ms: int = 0,
        abort: threading.Event | None = None,
    ) -> None: ...
    def map(
        self,
        url: str,
        *,
        limit: int = 5000,
        include: str | list[str] | None = None,
        exclude: str | list[str] | None = None,
    ) -> list[MappedUrl]: ...

class ServoFetchError(Exception): ...
class InvalidUrlError(ServoFetchError): ...
class FetchTimeoutError(ServoFetchError): ...
class NetworkError(ServoFetchError): ...
class EngineError(ServoFetchError): ...
class SchemaError(ServoFetchError): ...

def fetch(
    url: str,
    *,
    timeout: float | None = None,
    settle: float | None = None,
    user_agent: str | None = None,
    screenshot: bool = False,
    javascript: str | None = None,
    schema: Schema | None = None,
) -> Page: ...
