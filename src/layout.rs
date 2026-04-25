//! CSS layout heuristics — detects page structure (navbar, sidebar, footer, main)
//! to improve content extraction accuracy.

use serde::Deserialize;

/// Default viewport width used by Servo for rendering.
pub const VIEWPORT_WIDTH: u32 = 1280;
/// Default viewport height used by Servo for rendering.
pub const VIEWPORT_HEIGHT: u32 = 800;

/// A page element with CSS layout data, deserialized from the injected JS.
///
/// The `tag` field contains the HTML tag name in uppercase, as returned by
/// `Element.tagName` in JavaScript.
#[derive(Deserialize)]
pub struct LayoutElement {
    tag: String,
    role: Option<String>,
    w: f64,
    h: f64,
    position: String,
}

impl LayoutElement {
    fn is_navbar(&self) -> bool {
        matches!(self.position.as_str(), "fixed" | "sticky") && self.h < f64::from(VIEWPORT_HEIGHT) * 0.2
    }

    fn is_sidebar(&self) -> bool {
        let is_narrow = self.w < f64::from(VIEWPORT_WIDTH) * 0.3;
        let is_side_tag = matches!(self.tag.as_str(), "ASIDE" | "NAV");
        let is_side_role = matches!(self.role.as_deref(), Some("navigation" | "complementary"));
        is_narrow && (is_side_tag || is_side_role)
    }

    fn is_footer(&self) -> bool {
        let is_full_width = self.w >= f64::from(VIEWPORT_WIDTH) * 0.8;
        (self.tag == "FOOTER" && is_full_width) || self.role.as_deref() == Some("contentinfo")
    }

    fn should_remove(&self) -> bool {
        self.is_navbar() || self.is_sidebar() || self.is_footer()
    }
}

/// CSS selectors for elements that should be stripped before passing to readability.
///
/// Each selector is as specific as possible to avoid removing unrelated elements
/// that happen to share the same tag name.
#[must_use]
pub fn selectors_to_strip(elements: &[LayoutElement]) -> Vec<String> {
    let mut sels: Vec<String> = elements
        .iter()
        .filter(|el| el.should_remove())
        .map(|el| {
            let tag = el.tag.to_lowercase();
            match el.role.as_deref() {
                Some(role) => format!("{tag}[role=\"{role}\"]"),
                None => tag,
            }
        })
        .collect();
    sels.sort_unstable();
    sels.dedup();
    sels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(tag: &str, w: f64, h: f64, position: &str, role: Option<&str>) -> LayoutElement {
        LayoutElement {
            tag: tag.to_string(),
            role: role.map(String::from),
            w,
            h,
            position: position.to_string(),
        }
    }

    #[test]
    fn detects_fixed_navbar() {
        let sels = selectors_to_strip(&[el("HEADER", 1280.0, 60.0, "fixed", None)]);
        assert_eq!(sels, vec!["header"]);
    }

    #[test]
    fn detects_sticky_navbar() {
        let sels = selectors_to_strip(&[el("NAV", 1280.0, 50.0, "sticky", None)]);
        assert_eq!(sels, vec!["nav"]);
    }

    #[test]
    fn ignores_tall_fixed_element() {
        let sels = selectors_to_strip(&[el("DIV", 1280.0, 400.0, "fixed", None)]);
        assert!(sels.is_empty());
    }

    #[test]
    fn detects_narrow_aside_as_sidebar() {
        let sels = selectors_to_strip(&[el("ASIDE", 300.0, 800.0, "static", None)]);
        assert_eq!(sels, vec!["aside"]);
    }

    #[test]
    fn detects_footer() {
        let sels = selectors_to_strip(&[el("FOOTER", 1280.0, 100.0, "static", None)]);
        assert_eq!(sels, vec!["footer"]);
    }

    #[test]
    fn ignores_narrow_footer() {
        // A <footer> inside an <article> is typically narrow — should not be stripped.
        let sels = selectors_to_strip(&[el("FOOTER", 600.0, 50.0, "static", None)]);
        assert!(sels.is_empty());
    }

    #[test]
    fn detects_contentinfo_role_as_footer() {
        let sels = selectors_to_strip(&[el("DIV", 1280.0, 100.0, "static", Some("contentinfo"))]);
        assert_eq!(sels, vec!["div[role=\"contentinfo\"]"]);
    }

    #[test]
    fn deduplicates_selectors() {
        let elements = vec![
            el("NAV", 200.0, 50.0, "fixed", None),
            el("NAV", 250.0, 40.0, "sticky", None),
        ];
        let sels = selectors_to_strip(&elements);
        assert_eq!(sels, vec!["nav"]);
    }

    #[test]
    fn detects_complementary_role_as_sidebar() {
        let sels = selectors_to_strip(&[el("DIV", 250.0, 800.0, "static", Some("complementary"))]);
        assert_eq!(sels, vec!["div[role=\"complementary\"]"]);
    }
}
