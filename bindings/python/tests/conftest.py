"""Shared fixtures and constants."""

from __future__ import annotations

import pytest

from servo_fetch import Field, Schema

PRODUCTS_HTML = """\
<html><body>
  <div class="product"><h2>Keyboard</h2><span class="price">$99</span><a href="/kbd">details</a></div>
  <div class="product"><h2>Mouse</h2><span class="price">$49</span><a href="/mouse">details</a></div>
</body></html>"""


@pytest.fixture
def products_html() -> str:
    return PRODUCTS_HTML


@pytest.fixture
def product_schema() -> Schema:
    return Schema(
        base_selector=".product",
        fields=[
            Field(name="title", selector="h2", type="text"),
            Field(name="price", selector=".price", type="text"),
            Field(name="url", selector="a", type="attribute", attribute="href"),
        ],
    )
