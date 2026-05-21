//! JSON payload reported by `js/visibility.js`.

use super::VisibilityFlags;

/// One element's report from the JS visibility pass.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct JsReport {
    pub(crate) id: String,
    #[serde(rename = "flags")]
    pub(crate) bits: u32,
}

impl JsReport {
    pub(crate) fn flags(&self) -> VisibilityFlags {
        VisibilityFlags::from_bits_truncate(self.bits)
    }
}

/// Parse the JSON payload returned by `js/visibility.js`.
pub(crate) fn parse_js_payload(payload: &str) -> Vec<JsReport> {
    serde_json::from_str(payload).unwrap_or_else(|err| {
        tracing::warn!(?err, "failed to parse visibility JS payload");
        Vec::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_report_decodes_flags_truncating_unknown_bits() {
        let r = JsReport {
            id: "1".into(),
            bits: 0xFFFF,
        };
        assert!(r.flags().contains(VisibilityFlags::OPACITY_ZERO));
    }

    #[test]
    fn parse_js_payload_returns_empty_on_invalid_json() {
        assert!(parse_js_payload("garbage").is_empty());
    }

    #[test]
    fn parse_js_payload_accepts_well_formed_input() {
        let reports = parse_js_payload(r#"[{"id":"1","flags":16},{"id":"2","flags":32}]"#);
        assert_eq!(reports.len(), 2);
        assert!(reports[0].flags().contains(VisibilityFlags::OPACITY_ZERO));
        assert!(reports[1].flags().contains(VisibilityFlags::CLIPPED));
    }
}
