import os
import threading
from collections.abc import Callable
from typing import Any, Literal, final

__version__: str

ConsoleLevel = Literal["log", "debug", "info", "warn", "error", "trace"]
FieldType = Literal["text", "attribute", "html", "inner_html", "nested_list"]

@final
class ConsoleMessage:
    @property
    def level(self) -> ConsoleLevel: ...
    @property
    def message(self) -> str: ...

@final
class Field:
    def __new__(
        cls,
        *,
        name: str,
        selector: str,
        type: FieldType,
        attribute: str | None = None,
        fields: list[Field] | None = None,
    ) -> Field: ...
    @property
    def name(self) -> str: ...
    @property
    def selector(self) -> str: ...
    @property
    def type(self) -> FieldType: ...

@final
class Schema:
    def __new__(
        cls,
        *,
        base_selector: str | None = None,
        fields: list[Field] = ...,
    ) -> Schema: ...
    @staticmethod
    def from_dict(data: dict[str, Any]) -> Schema: ...
    @staticmethod
    def from_json(json_str: str) -> Schema: ...
    @staticmethod
    def from_file(path: str | os.PathLike[str]) -> Schema: ...
    def extract(self, html: str) -> dict[str, Any] | list[dict[str, Any]]: ...

@final
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

@final
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

@final
class MappedUrl:
    @property
    def url(self) -> str: ...
    @property
    def lastmod(self) -> str | None: ...

@final
class Client:
    def __new__(
        cls,
        *,
        timeout: float = 30.0,
        settle: float = 0.0,
        user_agent: str | None = None,
    ) -> Client: ...
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
