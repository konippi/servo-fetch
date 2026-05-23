//! Visibility-aware extraction.

mod a11y;
mod js;
mod selectors;

use bitflags::bitflags;

pub(crate) use self::a11y::A11yIndex;
pub(crate) use self::selectors::selectors_to_strip;

/// User stylesheet applied before render to enforce ARIA, HTML, and modal
/// semantics so matched nodes never produce boxes.
pub(crate) const USER_STYLESHEET: &str = concat!(
    "[hidden] { display: none !important; }\n",
    "[aria-hidden=\"true\"] { display: none !important; }\n",
    "[role=\"dialog\"][aria-modal=\"true\"] { display: none !important; }\n",
    "[role=\"alertdialog\"] { display: none !important; }\n",
    "[role=\"tabpanel\"][aria-hidden=\"true\"] { display: none !important; }\n",
    "[aria-label*=\"cookie\" i], [aria-label*=\"consent\" i],\n",
    "[class*=\"cookie-banner\" i], [class*=\"cookie-consent\" i],\n",
    "[id*=\"cookie\" i][class*=\"banner\" i],\n",
    "[class*=\"newsletter-popup\" i], [class*=\"subscribe-modal\" i],\n",
    "#onetrust-banner-sdk, #onetrust-pc-sdk,\n",
    "#CybotCookiebotDialog, #CybotCookiebotDialogBodyUnderlay,\n",
    "#qc-cmp2-container, [id^=\"sp_message_container_\"],\n",
    "#didomi-host, #usercentrics-root,\n",
    "#truste-consent-track { display: none !important; }\n",
);

bitflags! {
    /// Reasons a DOM node may be considered hidden.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct VisibilityFlags: u32 {
        /// Box has zero or near-zero area (AccessKit bounds).
        const ZERO_SIZE                 = 1 << 0;
        /// Box positioned outside the viewport (AccessKit bounds).
        const OFFSCREEN                 = 1 << 1;
        /// Computed font-size below 1px (AccessKit).
        const FONT_SIZE_ZERO            = 1 << 2;
        /// Tab panel with `aria-selected="false"` (AccessKit).
        const TAB_PANEL_INACTIVE        = 1 << 3;
        /// Cumulative opacity below 0.01 (computed CSS via JS).
        const OPACITY_ZERO              = 1 << 4;
        /// `clip` or `clip-path` set to a fully-clipped value (computed CSS via JS).
        const CLIPPED                   = 1 << 5;
        /// `content-visibility: hidden` (computed CSS via JS).
        const CONTENT_VISIBILITY_HIDDEN = 1 << 6;
        /// `text-indent` below `-9999px` while box is otherwise visible.
        const TEXT_INDENT_OFFSCREEN     = 1 << 7;
        /// Likely a "screen reader only" pattern (1px clip absolute).
        const SR_ONLY                   = 1 << 8;
        /// `visibility: hidden` (computed CSS via JS).
        const VISIBILITY_HIDDEN         = 1 << 9;
    }
}

/// Controls which visibility violations result in nodes being stripped from
/// the extraction input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct VisibilityPolicy {
    pub(crate) strip_if_any: VisibilityFlags,
}

impl VisibilityPolicy {
    /// Strip CSS-, ARIA-, and geometry-hidden content while preserving sr-only.
    #[must_use]
    pub fn moderate() -> Self {
        Self {
            strip_if_any: VisibilityFlags::ZERO_SIZE
                | VisibilityFlags::OFFSCREEN
                | VisibilityFlags::FONT_SIZE_ZERO
                | VisibilityFlags::TAB_PANEL_INACTIVE
                | VisibilityFlags::OPACITY_ZERO
                | VisibilityFlags::CLIPPED
                | VisibilityFlags::CONTENT_VISIBILITY_HIDDEN
                | VisibilityFlags::TEXT_INDENT_OFFSCREEN
                | VisibilityFlags::VISIBILITY_HIDDEN,
        }
    }

    /// [`Self::moderate`] plus sr-only stripping.
    #[must_use]
    pub fn strict() -> Self {
        let mut p = Self::moderate();
        p.strip_if_any |= VisibilityFlags::SR_ONLY;
        p
    }

    /// Disable visibility-flag-based stripping.
    #[must_use]
    pub fn off() -> Self {
        Self {
            strip_if_any: VisibilityFlags::empty(),
        }
    }
}

impl Default for VisibilityPolicy {
    fn default() -> Self {
        Self::moderate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moderate_policy_strips_common_hides() {
        let p = VisibilityPolicy::moderate();
        assert!(p.strip_if_any.contains(VisibilityFlags::ZERO_SIZE));
        assert!(p.strip_if_any.contains(VisibilityFlags::OPACITY_ZERO));
        assert!(p.strip_if_any.contains(VisibilityFlags::TAB_PANEL_INACTIVE));
        assert!(!p.strip_if_any.contains(VisibilityFlags::SR_ONLY));
    }

    #[test]
    fn strict_policy_adds_sr_only() {
        let p = VisibilityPolicy::strict();
        assert!(p.strip_if_any.contains(VisibilityFlags::SR_ONLY));
    }

    #[test]
    fn off_policy_strips_nothing_directly() {
        let p = VisibilityPolicy::off();
        assert!(p.strip_if_any.is_empty());
    }

    #[test]
    fn default_is_moderate() {
        assert_eq!(
            VisibilityPolicy::default().strip_if_any,
            VisibilityPolicy::moderate().strip_if_any,
        );
    }

    #[test]
    fn user_stylesheet_targets_aria_hidden_and_hidden_attr() {
        assert!(USER_STYLESHEET.contains("[hidden]"));
        assert!(USER_STYLESHEET.contains("[aria-hidden=\"true\"]"));
        assert!(USER_STYLESHEET.contains("[role=\"dialog\"][aria-modal=\"true\"]"));
    }

    #[test]
    fn user_stylesheet_targets_major_cookie_consent_providers() {
        // Major GDPR / CCPA cookie consent platforms by ID.
        // Adding new providers here requires no other changes — render-time hide
        // surfaces them via the `OPACITY_ZERO` flag in visibility.js, and they
        // are stripped by `moderate` policy.
        assert!(USER_STYLESHEET.contains("#onetrust-banner-sdk"));
        assert!(USER_STYLESHEET.contains("#CybotCookiebotDialog"));
        assert!(USER_STYLESHEET.contains("#qc-cmp2-container"));
        assert!(USER_STYLESHEET.contains("#didomi-host"));
        assert!(USER_STYLESHEET.contains("#usercentrics-root"));
        assert!(USER_STYLESHEET.contains("[id^=\"sp_message_container_\"]"));
        assert!(USER_STYLESHEET.contains("#truste-consent-track"));
    }

    /// Guards bit-value synchronisation with `js/visibility.js`.
    #[test]
    fn js_flag_constants_match_rust() {
        assert_eq!(VisibilityFlags::OPACITY_ZERO.bits(), 1 << 4);
        assert_eq!(VisibilityFlags::CLIPPED.bits(), 1 << 5);
        assert_eq!(VisibilityFlags::CONTENT_VISIBILITY_HIDDEN.bits(), 1 << 6);
        assert_eq!(VisibilityFlags::TEXT_INDENT_OFFSCREEN.bits(), 1 << 7);
        assert_eq!(VisibilityFlags::SR_ONLY.bits(), 1 << 8);
        assert_eq!(VisibilityFlags::VISIBILITY_HIDDEN.bits(), 1 << 9);
    }
}
