//! CSS layout heuristics — detects page structure (navbar, sidebar, footer, main)
//! to improve content extraction accuracy.

use serde::Deserialize;

/// Default viewport width used by Servo for rendering.
pub const VIEWPORT_WIDTH: u32 = 1280;
/// Default viewport height used by Servo for rendering.
pub const VIEWPORT_HEIGHT: u32 = 800;

/// ARIA roles that influence our layout heuristics.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    Navigation,
    Complementary,
    Contentinfo,
    #[serde(untagged)]
    Other(String),
}

impl Role {
    fn as_str(&self) -> &str {
        match self {
            Self::Navigation => "navigation",
            Self::Complementary => "complementary",
            Self::Contentinfo => "contentinfo",
            Self::Other(s) => s,
        }
    }
}

/// CSS `position` values that we treat as stacking overlays (likely navbars).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Position {
    Fixed,
    Sticky,
    #[serde(other)]
    Other,
}

impl Position {
    fn is_stacking(&self) -> bool {
        matches!(self, Self::Fixed | Self::Sticky)
    }
}

/// A page element with CSS layout data, deserialized from the injected JS.
#[derive(Deserialize)]
pub struct LayoutElement {
    tag: String,
    role: Option<Role>,
    w: f64,
    h: f64,
    position: Position,
}

impl LayoutElement {
    fn is_navbar(&self) -> bool {
        self.position.is_stacking() && self.h < f64::from(VIEWPORT_HEIGHT) * 0.2
    }

    fn is_sidebar(&self) -> bool {
        let is_narrow = self.w < f64::from(VIEWPORT_WIDTH) * 0.3;
        let is_side_tag = matches!(self.tag.as_str(), "ASIDE" | "NAV");
        let is_side_role = matches!(self.role, Some(Role::Navigation | Role::Complementary));
        is_narrow && (is_side_tag || is_side_role)
    }

    fn is_footer(&self) -> bool {
        let is_full_width = self.w >= f64::from(VIEWPORT_WIDTH) * 0.8;
        (self.tag == "FOOTER" && is_full_width) || self.role == Some(Role::Contentinfo)
    }

    fn should_remove(&self) -> bool {
        self.is_navbar() || self.is_sidebar() || self.is_footer()
    }
}

/// CSS selectors for elements that should be stripped before passing to readability.
#[must_use]
pub fn selectors_to_strip(elements: &[LayoutElement]) -> Vec<String> {
    let mut sels: Vec<String> = elements
        .iter()
        .filter(|el| el.should_remove())
        .map(|el| {
            let tag = el.tag.to_lowercase();
            match &el.role {
                Some(role) => format!("{tag}[role=\"{}\"]", role.as_str()),
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

    fn el(tag: &str, w: f64, h: f64, position: Position, role: Option<Role>) -> LayoutElement {
        LayoutElement {
            tag: tag.to_string(),
            role,
            w,
            h,
            position,
        }
    }

    #[test]
    fn detects_fixed_navbar() {
        let sels = selectors_to_strip(&[el("HEADER", 1280.0, 60.0, Position::Fixed, None)]);
        assert_eq!(sels, vec!["header"]);
    }

    #[test]
    fn detects_sticky_navbar() {
        let sels = selectors_to_strip(&[el("NAV", 1280.0, 50.0, Position::Sticky, None)]);
        assert_eq!(sels, vec!["nav"]);
    }

    #[test]
    fn ignores_tall_fixed_element() {
        let sels = selectors_to_strip(&[el("DIV", 1280.0, 400.0, Position::Fixed, None)]);
        assert!(sels.is_empty());
    }

    #[test]
    fn detects_narrow_aside_as_sidebar() {
        let sels = selectors_to_strip(&[el("ASIDE", 300.0, 800.0, Position::Other, None)]);
        assert_eq!(sels, vec!["aside"]);
    }

    #[test]
    fn detects_footer() {
        let sels = selectors_to_strip(&[el("FOOTER", 1280.0, 100.0, Position::Other, None)]);
        assert_eq!(sels, vec!["footer"]);
    }

    #[test]
    fn ignores_narrow_footer() {
        // A <footer> inside an <article> is typically narrow — should not be stripped.
        let sels = selectors_to_strip(&[el("FOOTER", 600.0, 50.0, Position::Other, None)]);
        assert!(sels.is_empty());
    }

    #[test]
    fn detects_contentinfo_role_as_footer() {
        let sels = selectors_to_strip(&[el("DIV", 1280.0, 100.0, Position::Other, Some(Role::Contentinfo))]);
        assert_eq!(sels, vec!["div[role=\"contentinfo\"]"]);
    }

    #[test]
    fn deduplicates_selectors() {
        let elements = vec![
            el("NAV", 200.0, 50.0, Position::Fixed, None),
            el("NAV", 250.0, 40.0, Position::Sticky, None),
        ];
        let sels = selectors_to_strip(&elements);
        assert_eq!(sels, vec!["nav"]);
    }

    #[test]
    fn detects_complementary_role_as_sidebar() {
        let sels = selectors_to_strip(&[el("DIV", 250.0, 800.0, Position::Other, Some(Role::Complementary))]);
        assert_eq!(sels, vec!["div[role=\"complementary\"]"]);
    }

    #[test]
    fn deserializes_role_from_json() {
        let el: LayoutElement =
            serde_json::from_str(r#"{"tag":"DIV","role":"navigation","w":100.0,"h":50.0,"position":"static"}"#)
                .unwrap();
        assert_eq!(el.role, Some(Role::Navigation));
    }

    #[test]
    fn deserializes_unknown_role_as_other() {
        let el: LayoutElement =
            serde_json::from_str(r#"{"tag":"DIV","role":"banner","w":100.0,"h":50.0,"position":"static"}"#).unwrap();
        assert_eq!(el.role, Some(Role::Other("banner".to_string())));
    }

    #[test]
    fn deserializes_unknown_position_as_other() {
        let el: LayoutElement =
            serde_json::from_str(r#"{"tag":"DIV","role":null,"w":100.0,"h":50.0,"position":"absolute"}"#).unwrap();
        assert_eq!(el.position, Position::Other);
    }

    #[test]
    fn navigation_role_sidebar() {
        let sels = selectors_to_strip(&[el("NAV", 250.0, 800.0, Position::Other, Some(Role::Navigation))]);
        assert_eq!(sels, vec!["nav[role=\"navigation\"]"]);
    }

    #[test]
    fn empty_elements_returns_empty() {
        let sels = selectors_to_strip(&[]);
        assert!(sels.is_empty());
    }
}
