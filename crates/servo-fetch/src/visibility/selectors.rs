//! CSS selector synthesis for visibility-driven stripping.

use std::collections::BTreeSet;

use super::VisibilityPolicy;
use super::a11y::A11yIndex;
use super::js::parse_js_payload;

/// DOM mirror of [`super::USER_STYLESHEET`].
const ALWAYS_STRIP_SELECTORS: &[&str] = &[
    "[hidden]",
    "[aria-hidden=\"true\"]",
    "[role=\"dialog\"][aria-modal=\"true\"]",
    "[role=\"alertdialog\"]",
    "[role=\"tabpanel\"][aria-hidden=\"true\"]",
];

/// Compute the CSS selectors to strip, merging policy, A11y, and JS signals.
pub(crate) fn selectors_to_strip(
    policy: VisibilityPolicy,
    a11y: Option<&A11yIndex<'_>>,
    js_payload: Option<&str>,
) -> Vec<String> {
    let mut out: BTreeSet<String> = ALWAYS_STRIP_SELECTORS.iter().map(|s| (*s).to_owned()).collect();

    if let Some(a11y) = a11y {
        out.extend(a11y.boilerplate_selectors());
        out.extend(a11y.flagged_selectors(policy));
    }

    if let Some(payload) = js_payload {
        for report in parse_js_payload(payload) {
            if policy.strip_if_any.intersects(report.flags()) {
                out.insert(data_vf_id_selector(&report.id));
            }
        }
    }

    out.into_iter().collect()
}

/// Build a `[data-vf-id="..."]` selector with the value safely escaped.
fn data_vf_id_selector(id: &str) -> String {
    let mut sel = String::with_capacity("[data-vf-id=]".len() + id.len() + 2);
    sel.push_str("[data-vf-id=");
    cssparser::serialize_string(id, &mut sel).expect("write to String never fails");
    sel.push(']');
    sel
}

/// Bytes one CSS hex escape adds.
const ESCAPE_MARGIN: usize = 4;

/// Build a CSS selector from an HTML tag and an optional class attribute value.
pub(super) fn make_selector(tag: &str, class: Option<&str>) -> String {
    let classes: Vec<&str> = class.map(|c| c.split_whitespace().collect()).unwrap_or_default();
    let extra: usize = classes.iter().map(|c| 1 + c.len() + ESCAPE_MARGIN).sum();
    let mut out = String::with_capacity(tag.len() + extra);
    out.push_str(tag);
    out.make_ascii_lowercase();
    for cls in classes {
        out.push('.');
        cssparser::serialize_identifier(cls, &mut out).expect("String write never fails");
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use servo::accesskit::{Node, NodeId, Role};

    use super::*;

    #[test]
    fn returns_data_vf_id_for_matching_flags() {
        let policy = VisibilityPolicy::moderate();
        let payload = r#"[{"id":"1","flags":16}]"#;
        let sels = selectors_to_strip(policy, None, Some(payload));
        assert!(sels.contains(&"[data-vf-id=\"1\"]".to_owned()));
    }

    #[test]
    fn skips_flags_outside_policy() {
        let policy = VisibilityPolicy::moderate();
        let payload = r#"[{"id":"1","flags":256}]"#;
        let sels = selectors_to_strip(policy, None, Some(payload));
        assert!(!sels.iter().any(|s| s.contains("data-vf-id")));
    }

    #[test]
    fn includes_boilerplate_when_a11y_provided() {
        let mut nodes = HashMap::new();
        let mut nav = Node::new(Role::Navigation);
        nav.set_html_tag("nav");
        nav.set_class_name("site-nav");
        nodes.insert(NodeId(1), nav);
        let index = A11yIndex::new(&nodes);
        let policy = VisibilityPolicy::moderate();
        let sels = selectors_to_strip(policy, Some(&index), None);
        assert!(sels.contains(&"nav.site-nav".to_owned()));
    }

    #[test]
    fn always_strip_selectors_present_regardless_of_inputs() {
        let policy = VisibilityPolicy::off();
        let sels = selectors_to_strip(policy, None, None);
        assert!(sels.contains(&"[hidden]".to_owned()));
        assert!(sels.contains(&"[aria-hidden=\"true\"]".to_owned()));
        assert!(sels.contains(&"[role=\"dialog\"][aria-modal=\"true\"]".to_owned()));
    }

    #[test]
    fn malicious_js_id_is_escaped_within_selector_value() {
        let policy = VisibilityPolicy::moderate();
        let payload = r#"[{"id":"1\"]:has(*)","flags":16}]"#;
        let sels = selectors_to_strip(policy, None, Some(payload));
        let injected = sels.iter().find(|s| s.contains("data-vf-id")).unwrap();
        assert!(injected.contains(r#"\""#), "unescaped quote in selector: {injected}");
        assert!(injected.starts_with("[data-vf-id="));
        assert!(injected.ends_with(']'));
        let matcher = dom_query::Matcher::new(injected).expect("selector parses");
        let doc = dom_query::Document::from(r#"<p data-vf-id="1">x</p>"#);
        assert!(doc.select_matcher(&matcher).is_empty());
    }

    #[test]
    fn make_selector_uses_all_classes_for_specificity() {
        assert_eq!(make_selector("DIV", Some("a b c")), "div.a.b.c");
        assert_eq!(make_selector("nav", Some("site-nav main")), "nav.site-nav.main");
    }

    #[test]
    fn make_selector_falls_back_to_tag_when_no_class() {
        assert_eq!(make_selector("FOOTER", None), "footer");
        assert_eq!(make_selector("FOOTER", Some("")), "footer");
        assert_eq!(make_selector("FOOTER", Some("   ")), "footer");
    }

    #[test]
    fn make_selector_escapes_special_chars_in_class() {
        assert_eq!(make_selector("div", Some("foo:bar")), r"div.foo\:bar");
        assert_eq!(make_selector("div", Some("normal-class_1")), "div.normal-class_1");
    }

    #[test]
    fn make_selector_escapes_leading_digit_in_class() {
        assert_eq!(make_selector("div", Some("3col")), r"div.\33 col");
        assert_eq!(make_selector("div", Some("9")), r"div.\39 ");
    }
}
