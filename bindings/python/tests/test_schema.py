"""Schema extraction — offline, no network required."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from servo_fetch import Field, Schema, SchemaError


class TestFromDict:
    def test_extracts_all_fields(self, product_schema, products_html) -> None:
        data = product_schema.extract(products_html)
        assert data == [
            {"title": "Keyboard", "price": "$99", "url": "/kbd"},
            {"title": "Mouse", "price": "$49", "url": "/mouse"},
        ]

    def test_from_json_equivalent(self, product_schema, products_html) -> None:
        schema = Schema.from_json(
            json.dumps(
                {
                    "base_selector": ".product",
                    "fields": [
                        {"name": "title", "selector": "h2", "type": "text"},
                        {"name": "price", "selector": ".price", "type": "text"},
                        {"name": "url", "selector": "a", "type": "attribute", "attribute": "href"},
                    ],
                }
            )
        )
        assert schema.extract(products_html) == product_schema.extract(products_html)


class TestFromFile:
    def test_pathlib_path(self, tmp_path: Path) -> None:
        p = tmp_path / "schema.json"
        p.write_text('{"fields":[{"name":"t","selector":"h1","type":"text"}]}')
        assert Schema.from_file(p).extract("<h1>Y</h1>") == {"t": "Y"}

    def test_str_path(self, tmp_path: Path) -> None:
        p = tmp_path / "schema.json"
        p.write_text('{"fields":[{"name":"t","selector":"h1","type":"text"}]}')
        assert Schema.from_file(str(p)).extract("<h1>Y</h1>") == {"t": "Y"}


class TestKwargsConstructor:
    def test_basic(self, products_html) -> None:
        schema = Schema(
            base_selector=".product",
            fields=[
                Field(name="title", selector="h2", type="text"),
                Field(name="price", selector=".price", type="text"),
                Field(name="url", selector="a", type="attribute", attribute="href"),
            ],
        )
        assert schema.extract(products_html) == [
            {"title": "Keyboard", "price": "$99", "url": "/kbd"},
            {"title": "Mouse", "price": "$49", "url": "/mouse"},
        ]

    def test_nested_list(self) -> None:
        html = "<ul><li><span>a</span><em>1</em></li><li><span>b</span><em>2</em></li></ul>"
        schema = Schema(
            fields=[
                Field(
                    name="items",
                    selector="li",
                    type="nested_list",
                    fields=[
                        Field(name="label", selector="span", type="text"),
                        Field(name="num", selector="em", type="text"),
                    ],
                ),
            ],
        )
        assert schema.extract(html) == {"items": [{"label": "a", "num": "1"}, {"label": "b", "num": "2"}]}


class TestFieldValidation:
    def test_attribute_requires_kwarg(self) -> None:
        with pytest.raises(ValueError, match="attribute"):
            Field(name="url", selector="a", type="attribute")

    def test_nested_list_requires_fields(self) -> None:
        with pytest.raises(ValueError, match="fields"):
            Field(name="items", selector=".item", type="nested_list")

    def test_unknown_type_rejected(self) -> None:
        with pytest.raises(ValueError, match="unknown type"):
            Field(name="x", selector="h1", type="bogus")


class TestErrors:
    def test_invalid_selector(self) -> None:
        with pytest.raises(SchemaError, match="invalid CSS selector"):
            Schema.from_dict({"fields": [{"name": "x", "selector": "###bad[[", "type": "text"}]})

    def test_malformed_json(self) -> None:
        with pytest.raises(SchemaError):
            Schema.from_json("{not json")

    def test_file_not_found(self, tmp_path: Path) -> None:
        with pytest.raises(SchemaError):
            Schema.from_file(tmp_path / "nonexistent.json")


class TestEdgeCases:
    @pytest.mark.parametrize(
        ("html", "expected"),
        [
            ("<html></html>", {"x": None}),
            ("", {"x": None}),
            ("<html><body><h1></h1></body></html>", {"x": ""}),
            ("<h1>日本語 🦀</h1>", {"x": "日本語 🦀"}),
            ("<h1>A &amp; B &lt; C</h1>", {"x": "A & B < C"}),
        ],
        ids=["empty_html", "blank_string", "empty_element", "unicode", "html_entities"],
    )
    def test_extract_edge_html(self, html: str, expected: dict) -> None:
        schema = Schema.from_dict({"fields": [{"name": "x", "selector": "h1", "type": "text"}]})
        assert schema.extract(html) == expected

    def test_base_selector_no_match_yields_empty_array(self) -> None:
        schema = Schema(base_selector=".nope", fields=[Field(name="t", selector="h1", type="text")])
        assert schema.extract("<html></html>") == []

    def test_self_selector_reads_matched_element(self) -> None:
        schema = Schema(base_selector="li", fields=[Field(name="t", selector="", type="text")])
        assert schema.extract("<ul><li>a</li><li>b</li></ul>") == [{"t": "a"}, {"t": "b"}]


class TestRoundtrip:
    def test_from_dict_roundtrip(self, product_schema, products_html) -> None:
        """Extract, then re-create schema from same dict — results identical."""
        result1 = product_schema.extract(products_html)
        schema2 = Schema.from_dict(
            {
                "base_selector": ".product",
                "fields": [
                    {"name": "title", "selector": "h2", "type": "text"},
                    {"name": "price", "selector": ".price", "type": "text"},
                    {"name": "url", "selector": "a", "type": "attribute", "attribute": "href"},
                ],
            }
        )
        assert schema2.extract(products_html) == result1


class TestRepr:
    def test_schema_repr(self) -> None:
        s = Schema(fields=[Field(name="x", selector="h1", type="text")])
        assert repr(s).startswith("Schema(fields=")

    def test_field_repr(self) -> None:
        f = Field(name="x", selector="h1", type="text")
        assert repr(f).startswith("Field(")
