//! CSS-selector schema extraction.

use dom_query::{Document, Matcher, Selection};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Maximum `NestedList` nesting depth allowed in a schema.
const MAX_NESTING_DEPTH: usize = 64;

/// Schema parse or validation error.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SchemaError {
    /// A CSS selector failed to parse.
    #[error("invalid CSS selector '{selector}' in field '{field}'")]
    InvalidSelector {
        /// Field whose selector failed.
        field: String,
        /// The offending selector text.
        selector: String,
    },
    /// The schema JSON itself is malformed.
    #[error("failed to parse schema: {0}")]
    Parse(#[from] serde_json::Error),
    /// Failed to read the schema file from disk.
    #[error("failed to read schema file: {0}")]
    Io(#[from] std::io::Error),
    /// Schema nesting exceeds [`MAX_NESTING_DEPTH`].
    #[error("schema nesting too deep at field '{field}' ({depth} levels, max {max})")]
    TooDeep {
        /// Dotted path to the field that tripped the limit.
        field: String,
        /// Depth at which the limit was tripped.
        depth: usize,
        /// Maximum depth allowed.
        max: usize,
    },
}

/// Declarative extraction schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExtractSchema {
    /// Repeated container selector; each match produces one object.
    #[serde(default, alias = "baseSelector")]
    pub(crate) base_selector: Option<String>,
    /// Fields to read from each container.
    pub(crate) fields: Vec<ExtractField>,
}

impl ExtractSchema {
    /// Parse a schema from JSON and validate every selector eagerly.
    pub fn from_json(json: &str) -> Result<Self, SchemaError> {
        let schema: Self = serde_json::from_str(json)?;
        schema.validate()?;
        Ok(schema)
    }

    /// Load a schema from a JSON file on disk.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, SchemaError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_json(&content)
    }

    /// Start building a schema programmatically.
    #[must_use]
    pub fn builder() -> SchemaBuilder {
        SchemaBuilder::default()
    }

    /// Validate every selector in the schema (including nested fields).
    pub fn validate(&self) -> Result<(), SchemaError> {
        if let Some(sel) = &self.base_selector {
            check_selector("<base>", sel)?;
        }
        for f in &self.fields {
            f.validate("", 0)?;
        }
        Ok(())
    }

    /// Repeated container selector, if any.
    #[must_use]
    pub fn base_selector(&self) -> Option<&str> {
        self.base_selector.as_deref()
    }

    /// Fields defined in this schema.
    #[must_use]
    pub fn fields(&self) -> &[ExtractField] {
        &self.fields
    }
}

/// A single field in an [`ExtractSchema`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExtractField {
    /// Output key for this field.
    pub(crate) name: String,
    /// CSS selector relative to the current container.
    pub(crate) selector: String,
    /// How to extract the value.
    #[serde(flatten)]
    pub(crate) kind: FieldKind,
}

impl ExtractField {
    /// Construct a field programmatically.
    pub fn new(name: impl Into<String>, selector: impl Into<String>, kind: FieldKind) -> Self {
        Self {
            name: name.into(),
            selector: selector.into(),
            kind,
        }
    }

    /// Output key name for this field.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// CSS selector for this field.
    #[must_use]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// Extraction kind.
    #[must_use]
    pub fn kind(&self) -> &FieldKind {
        &self.kind
    }

    fn validate(&self, parent: &str, depth: usize) -> Result<(), SchemaError> {
        let path = if parent.is_empty() {
            self.name.clone()
        } else {
            format!("{parent}.{}", self.name)
        };
        if depth > MAX_NESTING_DEPTH {
            return Err(SchemaError::TooDeep {
                field: path,
                depth,
                max: MAX_NESTING_DEPTH,
            });
        }
        check_selector(&path, &self.selector)?;
        if let FieldKind::NestedList { fields } = &self.kind {
            for f in fields {
                f.validate(&path, depth + 1)?;
            }
        }
        Ok(())
    }
}

/// Builder for [`ExtractSchema`].
#[derive(Default, Debug, Clone)]
pub struct SchemaBuilder {
    base_selector: Option<String>,
    fields: Vec<ExtractField>,
}

impl SchemaBuilder {
    /// Set the base (repeated container) selector.
    #[must_use]
    pub fn base_selector(mut self, selector: impl Into<String>) -> Self {
        self.base_selector = Some(selector.into());
        self
    }

    /// Add a field. Accepts any [`FieldKind`] variant.
    #[must_use]
    pub fn field(mut self, name: impl Into<String>, selector: impl Into<String>, kind: FieldKind) -> Self {
        self.fields.push(ExtractField::new(name, selector, kind));
        self
    }

    /// Finalize the schema, validating every selector eagerly.
    pub fn build(self) -> Result<ExtractSchema, SchemaError> {
        let schema = ExtractSchema {
            base_selector: self.base_selector,
            fields: self.fields,
        };
        schema.validate()?;
        Ok(schema)
    }
}

/// What to read once a field selector matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum FieldKind {
    /// Descendant text of the first match.
    Text,
    /// Named attribute on the first match.
    #[serde(alias = "attr")]
    Attribute {
        /// Attribute name to read (e.g. `href`).
        attribute: String,
    },
    /// Outer HTML of the first match.
    Html,
    /// Inner HTML of the first match.
    #[serde(alias = "innerHtml")]
    InnerHtml,
    /// Repeated sub-object per match, using nested field definitions.
    #[serde(alias = "nestedList")]
    NestedList {
        /// Nested field definitions.
        fields: Vec<ExtractField>,
    },
}

fn check_selector(field: &str, selector: &str) -> Result<(), SchemaError> {
    // Empty selector is a sentinel for "the matched element itself" and is
    // intentionally not a valid CSS expression; skip parsing it.
    if selector.is_empty() {
        return Ok(());
    }
    Matcher::new(selector)
        .map(|_| ())
        .map_err(|_| SchemaError::InvalidSelector {
            field: field.to_string(),
            selector: selector.to_string(),
        })
}

impl ExtractSchema {
    /// Apply this schema to HTML, returning structured JSON.
    #[must_use]
    pub fn extract_from(&self, html: &str) -> Value {
        let doc = Document::from(html);
        let root = doc.select("html");

        match &self.base_selector {
            None => Value::Object(extract_fields(&root, &self.fields)),
            Some(sel) => {
                let items: Vec<Value> = doc
                    .select(sel)
                    .iter()
                    .map(|container| Value::Object(extract_fields(&container, &self.fields)))
                    .collect();
                Value::Array(items)
            }
        }
    }
}

fn extract_fields(container: &Selection<'_>, fields: &[ExtractField]) -> Map<String, Value> {
    fields
        .iter()
        .map(|f| (f.name.clone(), extract_field(container, f)))
        .collect()
}

fn extract_field(container: &Selection<'_>, field: &ExtractField) -> Value {
    // An empty selector is a sentinel for "the matched element itself".
    let picked = if field.selector.is_empty() {
        container.clone()
    } else {
        container.select(&field.selector)
    };
    if !picked.exists() {
        return Value::Null;
    }
    // Scalar kinds read the first match; dom_query concatenates across all matches by default.
    match &field.kind {
        FieldKind::Text => Value::String(picked.first().text().to_string()),
        FieldKind::Attribute { attribute } => picked
            .first()
            .attr(attribute)
            .map_or(Value::Null, |s| Value::String(s.to_string())),
        FieldKind::Html => Value::String(picked.first().html().to_string()),
        FieldKind::InnerHtml => Value::String(picked.first().inner_html().to_string()),
        FieldKind::NestedList { fields } => Value::Array(
            picked
                .iter()
                .map(|sub| Value::Object(extract_fields(&sub, fields)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PRODUCTS: &str = r#"
        <html><body>
          <div class="product">
            <h2>Keyboard</h2>
            <span class="price">$99</span>
            <a href="/kbd">details</a>
            <img src="/kbd.png" alt="Keyboard">
          </div>
          <div class="product">
            <h2>Mouse</h2>
            <span class="price">$49</span>
            <a href="/mouse">details</a>
            <img src="/mouse.png" alt="Mouse">
          </div>
        </body></html>
    "#;

    fn schema_from(json: &Value) -> ExtractSchema {
        ExtractSchema::from_json(&json.to_string()).expect("valid schema")
    }

    #[test]
    fn extracts_text_fields_over_base_selector() {
        let schema = schema_from(&json!({
            "base_selector": ".product",
            "fields": [
                { "name": "title", "selector": "h2", "type": "text" },
                { "name": "price", "selector": ".price", "type": "text" },
            ]
        }));
        assert_eq!(
            schema.extract_from(PRODUCTS),
            json!([
                { "title": "Keyboard", "price": "$99" },
                { "title": "Mouse", "price": "$49" }
            ])
        );
    }

    #[test]
    fn extracts_attribute() {
        let schema = schema_from(&json!({
            "base_selector": ".product",
            "fields": [
                { "name": "url", "selector": "a", "type": "attribute", "attribute": "href" },
                { "name": "image", "selector": "img", "type": "attribute", "attribute": "src" },
            ]
        }));
        assert_eq!(
            schema.extract_from(PRODUCTS),
            json!([
                { "url": "/kbd", "image": "/kbd.png" },
                { "url": "/mouse", "image": "/mouse.png" }
            ])
        );
    }

    #[test]
    fn extracts_html_and_inner_html() {
        let html = r#"<html><body><div class="card"><p><b>hi</b></p></div></body></html>"#;
        let schema = schema_from(&json!({
            "base_selector": ".card",
            "fields": [
                { "name": "outer", "selector": "p", "type": "html" },
                { "name": "inner", "selector": "p", "type": "inner_html" },
            ]
        }));
        assert_eq!(
            schema.extract_from(html),
            json!([{ "outer": "<p><b>hi</b></p>", "inner": "<b>hi</b>" }])
        );
    }

    #[test]
    fn nested_list_extracts_sub_objects() {
        let html = r#"
            <html><body>
              <div class="post">
                <h3>First</h3>
                <ul><li>a</li><li>b</li></ul>
              </div>
              <div class="post">
                <h3>Second</h3>
                <ul><li>c</li></ul>
              </div>
            </body></html>
        "#;
        let schema = schema_from(&json!({
            "base_selector": ".post",
            "fields": [
                { "name": "title", "selector": "h3", "type": "text" },
                { "name": "items", "selector": "li", "type": "nested_list",
                  "fields": [
                    { "name": "label", "selector": "*", "type": "text" }
                  ]
                }
            ]
        }));
        assert_eq!(
            schema.extract_from(html),
            json!([
                { "title": "First", "items": [{ "label": null }, { "label": null }] },
                { "title": "Second", "items": [{ "label": null }] }
            ])
        );
    }

    #[test]
    fn missing_field_yields_null() {
        let schema = schema_from(&json!({
            "base_selector": ".product",
            "fields": [
                { "name": "rating", "selector": ".rating", "type": "text" }
            ]
        }));
        assert_eq!(
            schema.extract_from(PRODUCTS),
            json!([{ "rating": null }, { "rating": null }])
        );
    }

    #[test]
    fn no_base_selector_returns_single_object() {
        let schema = schema_from(&json!({
            "fields": [
                { "name": "first_product", "selector": ".product h2", "type": "text" }
            ]
        }));
        assert_eq!(schema.extract_from(PRODUCTS), json!({ "first_product": "Keyboard" }));
    }

    #[test]
    fn accepts_camelcase_keys() {
        let schema = schema_from(&json!({
            "baseSelector": ".product",
            "fields": [
                { "name": "t", "selector": "h2", "type": "text" },
                { "name": "raw", "selector": "p", "type": "innerHtml" }
            ]
        }));
        assert_eq!(schema.base_selector.as_deref(), Some(".product"));
        let arr_out = schema.extract_from(PRODUCTS);
        let arr = arr_out.as_array().unwrap();
        assert_eq!(arr[0]["t"], "Keyboard");
        assert_eq!(arr[0]["raw"], Value::Null);
    }

    #[test]
    fn rejects_malformed_selector_eagerly() {
        let json = json!({
            "base_selector": ".product",
            "fields": [
                { "name": "bad", "selector": "###not[[[valid", "type": "text" }
            ]
        });
        let err = ExtractSchema::from_json(&json.to_string()).unwrap_err();
        assert!(
            matches!(err, SchemaError::InvalidSelector { field, .. } if field == "bad"),
            "expected InvalidSelector error for field 'bad'"
        );
    }

    #[test]
    fn nested_invalid_selector_reports_dotted_path() {
        let json = json!({
            "fields": [{
                "name": "products",
                "selector": ".product",
                "type": "nested_list",
                "fields": [{
                    "name": "price",
                    "selector": ".price",
                    "type": "nested_list",
                    "fields": [{ "name": "amount", "selector": "###bad", "type": "text" }]
                }]
            }]
        });
        let err = ExtractSchema::from_json(&json.to_string()).unwrap_err();
        assert!(
            matches!(&err, SchemaError::InvalidSelector { field, .. } if field == "products.price.amount"),
            "expected dotted path, got: {err:?}"
        );
    }

    #[test]
    fn rejects_malformed_json() {
        let err = ExtractSchema::from_json("{ not json").unwrap_err();
        assert!(matches!(err, SchemaError::Parse(_)), "expected Parse error");
    }

    #[test]
    fn from_path_surfaces_io_error() {
        let err = ExtractSchema::from_path("/definitely/not/a/real/path.json").unwrap_err();
        assert!(matches!(err, SchemaError::Io(_)), "expected Io error, got {err:?}");
    }

    #[test]
    fn mixed_present_and_missing_fields() {
        let schema = schema_from(&json!({
            "base_selector": ".product",
            "fields": [
                { "name": "title", "selector": "h2", "type": "text" },
                { "name": "rating", "selector": ".rating", "type": "text" }
            ]
        }));
        assert_eq!(
            schema.extract_from(PRODUCTS),
            json!([
                { "title": "Keyboard", "rating": null },
                { "title": "Mouse", "rating": null }
            ])
        );
    }

    #[test]
    fn empty_selector_reads_matched_element_text() {
        let html = r"<html><body><ul><li>alpha</li><li>beta</li></ul></body></html>";
        let schema = schema_from(&json!({
            "base_selector": "li",
            "fields": [
                { "name": "value", "selector": "", "type": "text" }
            ]
        }));
        assert_eq!(
            schema.extract_from(html),
            json!([{ "value": "alpha" }, { "value": "beta" }])
        );
    }

    #[test]
    fn empty_selector_inside_nested_list_reads_each_item() {
        let html = r#"
            <html><body>
              <div class="post">
                <h3>First</h3>
                <ul><li>a</li><li>b</li></ul>
              </div>
            </body></html>
        "#;
        let schema = schema_from(&json!({
            "base_selector": ".post",
            "fields": [
                { "name": "title", "selector": "h3", "type": "text" },
                { "name": "items", "selector": "li", "type": "nested_list",
                  "fields": [{ "name": "text", "selector": "", "type": "text" }] }
            ]
        }));
        assert_eq!(
            schema.extract_from(html),
            json!([{
                "title": "First",
                "items": [{ "text": "a" }, { "text": "b" }]
            }])
        );
    }

    #[test]
    fn empty_selector_reads_matched_element_attribute() {
        let html = r#"<html><body><a href="/home" title="Home">Go</a></body></html>"#;
        let schema = schema_from(&json!({
            "base_selector": "a",
            "fields": [
                { "name": "href", "selector": "", "type": "attribute", "attribute": "href" },
                { "name": "title", "selector": "", "type": "attribute", "attribute": "title" }
            ]
        }));
        assert_eq!(schema.extract_from(html), json!([{ "href": "/home", "title": "Home" }]));
    }

    #[test]
    fn builder_constructs_equivalent_schema() {
        let built = ExtractSchema::builder()
            .base_selector(".product")
            .field("title", "h2", FieldKind::Text)
            .field(
                "url",
                "a",
                FieldKind::Attribute {
                    attribute: "href".into(),
                },
            )
            .build()
            .unwrap();

        let json_schema = schema_from(&json!({
            "base_selector": ".product",
            "fields": [
                { "name": "title", "selector": "h2", "type": "text" },
                { "name": "url", "selector": "a", "type": "attribute", "attribute": "href" }
            ]
        }));

        assert_eq!(built.extract_from(PRODUCTS), json_schema.extract_from(PRODUCTS));
    }

    #[test]
    fn builder_supports_nested_list() {
        let schema = ExtractSchema::builder()
            .base_selector(".post")
            .field("title", "h3", FieldKind::Text)
            .field(
                "items",
                "li",
                FieldKind::NestedList {
                    fields: vec![ExtractField::new("text", "", FieldKind::Text)],
                },
            )
            .build()
            .unwrap();
        let html = r"<html><body><div class='post'><h3>A</h3><ul><li>one</li></ul></div></body></html>";
        assert_eq!(
            schema.extract_from(html),
            json!([{ "title": "A", "items": [{ "text": "one" }] }])
        );
    }

    #[test]
    fn builder_surfaces_selector_errors() {
        let err = ExtractSchema::builder()
            .field("bad", "###invalid[[[", FieldKind::Text)
            .build()
            .unwrap_err();
        assert!(
            matches!(&err, SchemaError::InvalidSelector { field, .. } if field == "bad"),
            "expected InvalidSelector, got {err:?}"
        );
    }

    #[test]
    fn ignores_unknown_top_level_fields() {
        let schema = schema_from(&json!({
            "name": "legacy-label",
            "base_selector": ".product",
            "fields": [
                { "name": "title", "selector": "h2", "type": "text" }
            ]
        }));
        assert_eq!(schema.base_selector.as_deref(), Some(".product"));
    }

    #[test]
    fn rejects_unknown_field_type_list() {
        let json = json!({
            "fields": [
                { "name": "items", "selector": "li", "type": "list", "fields": [] }
            ]
        });
        let err = ExtractSchema::from_json(&json.to_string()).unwrap_err();
        assert!(
            matches!(err, SchemaError::Parse(_)),
            "expected Parse error for unsupported 'list' type"
        );
    }

    #[test]
    fn works_on_html_fragment_without_wrappers() {
        let schema = schema_from(&json!({
            "fields": [
                { "name": "heading", "selector": "h1", "type": "text" }
            ]
        }));
        assert_eq!(schema.extract_from("<h1>Hello</h1>"), json!({ "heading": "Hello" }));
    }

    #[test]
    fn empty_fields_yields_empty_object() {
        let schema = schema_from(&json!({ "fields": [] }));
        assert_eq!(schema.extract_from(PRODUCTS), json!({}));
    }

    #[test]
    fn empty_fields_with_base_selector_yields_empty_objects() {
        let schema = schema_from(&json!({
            "base_selector": ".product",
            "fields": []
        }));
        assert_eq!(schema.extract_from(PRODUCTS), json!([{}, {}]));
    }

    #[test]
    fn base_selector_matches_nothing_yields_empty_array() {
        let schema = schema_from(&json!({
            "base_selector": ".does-not-exist",
            "fields": [
                { "name": "title", "selector": "h2", "type": "text" }
            ]
        }));
        assert_eq!(schema.extract_from(PRODUCTS), json!([]));
    }

    #[test]
    fn nested_list_with_zero_matches_yields_null() {
        let html = r#"<html><body><div class="post"><h3>Only</h3></div></body></html>"#;
        let schema = schema_from(&json!({
            "base_selector": ".post",
            "fields": [
                { "name": "title", "selector": "h3", "type": "text" },
                { "name": "items", "selector": ".missing", "type": "nested_list",
                  "fields": [{ "name": "label", "selector": "*", "type": "text" }] }
            ]
        }));
        assert_eq!(schema.extract_from(html), json!([{ "title": "Only", "items": null }]));
    }

    #[test]
    fn attribute_missing_but_element_present_yields_null() {
        let html = r"<html><body><a>no href</a></body></html>";
        let schema = schema_from(&json!({
            "fields": [
                { "name": "href", "selector": "a", "type": "attribute", "attribute": "href" }
            ]
        }));
        assert_eq!(schema.extract_from(html), json!({ "href": null }));
    }

    #[test]
    fn unicode_text_roundtrips() {
        let html = r"<html><body><h1>日本語 🦀</h1></body></html>";
        let schema = schema_from(&json!({
            "fields": [{ "name": "t", "selector": "h1", "type": "text" }]
        }));
        assert_eq!(schema.extract_from(html), json!({ "t": "日本語 🦀" }));
    }

    #[test]
    fn html_entities_are_decoded_in_text() {
        let html = r"<html><body><p>A &amp; B &lt; C</p></body></html>";
        let schema = schema_from(&json!({
            "fields": [{ "name": "t", "selector": "p", "type": "text" }]
        }));
        assert_eq!(schema.extract_from(html), json!({ "t": "A & B < C" }));
    }

    #[test]
    fn deeply_nested_three_levels() {
        let html = r#"
            <html><body>
              <div class="cat">
                <h2>Electronics</h2>
                <div class="prod">
                  <h3>Laptop</h3>
                  <ul class="specs"><li>16GB</li><li>1TB</li></ul>
                </div>
              </div>
            </body></html>
        "#;
        let schema = schema_from(&json!({
            "base_selector": ".cat",
            "fields": [
                { "name": "name", "selector": "h2", "type": "text" },
                { "name": "products", "selector": ".prod", "type": "nested_list",
                  "fields": [
                    { "name": "title", "selector": "h3", "type": "text" },
                    { "name": "specs", "selector": ".specs li", "type": "nested_list",
                      "fields": [{ "name": "v", "selector": "*", "type": "text" }] }
                  ] }
            ]
        }));
        assert_eq!(
            schema.extract_from(html),
            json!([{
                "name": "Electronics",
                "products": [{
                    "title": "Laptop",
                    "specs": [{ "v": null }, { "v": null }]
                }]
            }])
        );
    }

    #[test]
    fn empty_html_yields_nulls() {
        let schema = schema_from(&json!({
            "fields": [{ "name": "t", "selector": "h1", "type": "text" }]
        }));
        assert_eq!(schema.extract_from(""), json!({ "t": null }));
    }

    #[test]
    fn rejects_excessive_nesting_depth() {
        // Build a schema nested deeper than MAX_NESTING_DEPTH (64).
        let mut kind = FieldKind::Text;
        for i in (0..MAX_NESTING_DEPTH + 5).rev() {
            kind = FieldKind::NestedList {
                fields: vec![ExtractField::new(format!("l{i}"), "*", kind)],
            };
        }
        let err = ExtractSchema::builder().field("root", "*", kind).build().unwrap_err();
        assert!(matches!(
            err,
            SchemaError::TooDeep { depth, max, .. } if depth > max && max == MAX_NESTING_DEPTH
        ));
    }

    #[test]
    fn accepts_nesting_at_depth_limit() {
        // Build a schema exactly at MAX_NESTING_DEPTH nesting.
        let mut kind = FieldKind::Text;
        for i in (0..MAX_NESTING_DEPTH).rev() {
            kind = FieldKind::NestedList {
                fields: vec![ExtractField::new(format!("l{i}"), "*", kind)],
            };
        }
        let result = ExtractSchema::builder().field("root", "*", kind).build();
        assert!(result.is_ok());
    }

    #[test]
    fn accessors_expose_schema_contents() {
        let schema = ExtractSchema::builder()
            .base_selector(".product")
            .field("title", "h2", FieldKind::Text)
            .field(
                "url",
                "a",
                FieldKind::Attribute {
                    attribute: "href".into(),
                },
            )
            .build()
            .unwrap();

        assert_eq!(schema.base_selector(), Some(".product"));
        assert_eq!(schema.fields().len(), 2);
        assert_eq!(schema.fields()[0].name(), "title");
        assert_eq!(schema.fields()[0].selector(), "h2");
        assert!(matches!(schema.fields()[0].kind(), FieldKind::Text));
        assert_eq!(schema.fields()[1].name(), "url");
        assert!(matches!(
            schema.fields()[1].kind(),
            FieldKind::Attribute { attribute } if attribute == "href"
        ));
    }
}
