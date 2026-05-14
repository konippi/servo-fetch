//! `Schema` and `Field` pyclasses over `servo_fetch::schema`.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::errors::SchemaError;

/// A single field within a [`Schema`].
#[pyclass(frozen, module = "servo_fetch._native")]
pub(crate) struct Field {
    inner: servo_fetch::schema::ExtractField,
}

impl Field {
    fn inner(&self) -> &servo_fetch::schema::ExtractField {
        &self.inner
    }
}

#[pymethods]
impl Field {
    #[new]
    #[pyo3(signature = (*, name, selector, r#type, attribute=None, fields=None))]
    fn new(
        py: Python<'_>,
        name: String,
        selector: String,
        r#type: &str,
        attribute: Option<String>,
        fields: Option<Vec<Py<Field>>>,
    ) -> PyResult<Self> {
        use servo_fetch::schema::FieldKind;

        let kind = match r#type {
            "text" => FieldKind::Text,
            "html" => FieldKind::Html,
            "inner_html" | "innerHtml" => FieldKind::InnerHtml,
            "attribute" | "attr" => {
                let attr =
                    attribute.ok_or_else(|| PyValueError::new_err("type='attribute' requires attribute= kwarg"))?;
                FieldKind::Attribute { attribute: attr }
            }
            "nested_list" | "nestedList" => {
                let nested =
                    fields.ok_or_else(|| PyValueError::new_err("type='nested_list' requires fields= kwarg"))?;
                let inner_fields = nested.iter().map(|f| f.bind(py).borrow().inner.clone()).collect();
                FieldKind::NestedList { fields: inner_fields }
            }
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown type: {other:?} (expected text / html / inner_html / attribute / nested_list)"
                )));
            }
        };

        Ok(Self {
            inner: servo_fetch::schema::ExtractField::new(name, selector, kind),
        })
    }

    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    #[getter]
    fn selector(&self) -> &str {
        self.inner.selector()
    }

    #[getter]
    fn r#type(&self) -> &'static str {
        use servo_fetch::schema::FieldKind;
        match self.inner.kind() {
            FieldKind::Text => "text",
            FieldKind::Attribute { .. } => "attribute",
            FieldKind::Html => "html",
            FieldKind::InnerHtml => "inner_html",
            FieldKind::NestedList { .. } => "nested_list",
            _ => "unknown",
        }
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let name = slf.get_type().qualname()?;
        let this = slf.borrow();
        Ok(format!(
            "{name}(name={:?}, selector={:?}, type={:?})",
            this.inner.name(),
            this.inner.selector(),
            Field::kind_str(this.inner.kind())
        ))
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner.name() == other.inner.name()
            && self.inner.selector() == other.inner.selector()
            && Self::kind_str(self.inner.kind()) == Self::kind_str(other.inner.kind())
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.inner.name().hash(&mut h);
        self.inner.selector().hash(&mut h);
        Self::kind_str(self.inner.kind()).hash(&mut h);
        h.finish()
    }
}

impl Field {
    fn kind_str(kind: &servo_fetch::schema::FieldKind) -> &'static str {
        use servo_fetch::schema::FieldKind;
        match kind {
            FieldKind::Text => "text",
            FieldKind::Attribute { .. } => "attribute",
            FieldKind::Html => "html",
            FieldKind::InnerHtml => "inner_html",
            FieldKind::NestedList { .. } => "nested_list",
            _ => "unknown",
        }
    }
}

/// Declarative CSS-selector schema for structured extraction from rendered HTML.
#[pyclass(frozen, module = "servo_fetch._native")]
pub(crate) struct Schema {
    inner: servo_fetch::schema::ExtractSchema,
}

impl Schema {
    pub(crate) fn inner(&self) -> &servo_fetch::schema::ExtractSchema {
        &self.inner
    }
}

#[pymethods]
impl Schema {
    #[new]
    #[pyo3(signature = (*, base_selector=None, fields=vec![]))]
    fn __new__(py: Python<'_>, base_selector: Option<String>, fields: Vec<Py<Field>>) -> PyResult<Self> {
        let mut builder = servo_fetch::schema::ExtractSchema::builder();
        if let Some(bs) = base_selector {
            builder = builder.base_selector(bs);
        }
        for f in &fields {
            let borrowed = f.bind(py).borrow();
            let inner = borrowed.inner();
            builder = builder.field(inner.name(), inner.selector(), inner.kind().clone());
        }
        let inner = builder.build().map_err(|e| SchemaError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Build a schema from a Python dict.
    #[staticmethod]
    fn from_dict(data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let inner: servo_fetch::schema::ExtractSchema = pythonize::depythonize(data)?;
        inner.validate().map_err(|e| SchemaError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Build a schema from a JSON string.
    #[staticmethod]
    fn from_json(json_str: &str) -> PyResult<Self> {
        let inner =
            servo_fetch::schema::ExtractSchema::from_json(json_str).map_err(|e| SchemaError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Load a schema from a JSON file on disk. Accepts `str` or `os.PathLike`.
    #[staticmethod]
    fn from_file(py: Python<'_>, path: &Bound<'_, PyAny>) -> PyResult<Self> {
        let os = py.import("os")?;
        let path_str: String = os.call_method1("fspath", (path,))?.extract()?;
        let inner = servo_fetch::schema::ExtractSchema::from_path(&path_str)
            .map_err(|e| SchemaError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Apply this schema to HTML and return the extracted structure.
    fn extract<'py>(&self, py: Python<'py>, html: &str) -> PyResult<Bound<'py, PyAny>> {
        let value = py.detach(|| self.inner.extract_from(html));
        pythonize::pythonize(py, &value).map_err(Into::into)
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let name = slf.get_type().qualname()?;
        Ok(format!("{name}(fields={})", slf.borrow().inner.fields().len()))
    }
}
