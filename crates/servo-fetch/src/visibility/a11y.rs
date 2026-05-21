//! AccessKit tree indexing for visibility flags and boilerplate detection.

use std::collections::{BTreeSet, HashMap};

use servo::accesskit::{Node, NodeId, Rect, Role};

use super::{VisibilityFlags, VisibilityPolicy, selectors::make_selector};

/// Threshold matching common off-screen hide patterns.
const OFFSCREEN_THRESHOLD_PX: f64 = 99_999.0;

/// Indexed view of an AccessKit tree for boilerplate role and flag lookups.
#[derive(Debug)]
pub(crate) struct A11yIndex<'a> {
    nodes: &'a HashMap<NodeId, Node>,
    flags: HashMap<NodeId, VisibilityFlags>,
    by_role: HashMap<Role, Vec<NodeId>>,
}

impl<'a> A11yIndex<'a> {
    pub(crate) fn new(nodes: &'a HashMap<NodeId, Node>) -> Self {
        let mut flags = HashMap::new();
        let mut by_role: HashMap<Role, Vec<NodeId>> = HashMap::new();

        for (id, node) in nodes {
            by_role.entry(node.role()).or_default().push(*id);
            let f = compute_flags(node);
            if !f.is_empty() {
                flags.insert(*id, f);
            }
        }

        Self { nodes, flags, by_role }
    }

    /// CSS selectors targeting boilerplate roles in the tree.
    pub(crate) fn boilerplate_selectors(&self) -> BTreeSet<String> {
        const ROLES: &[Role] = &[Role::Navigation, Role::Banner, Role::ContentInfo, Role::Complementary];
        let mut out: BTreeSet<String> = ROLES
            .iter()
            .filter_map(|r| self.by_role.get(r))
            .flatten()
            .filter_map(|id| {
                let node = self.nodes.get(id)?;
                Some(make_selector(node.html_tag()?, node.class_name()))
            })
            .collect();

        if let Some(ids) = self.by_role.get(&Role::Search) {
            out.extend(ids.iter().filter_map(|id| {
                let node = self.nodes.get(id)?;
                let tag = node.html_tag()?;
                tag.eq_ignore_ascii_case("form")
                    .then(|| make_selector(tag, node.class_name()))
            }));
        }
        out
    }

    /// CSS selectors for nodes whose AccessKit-derived flags intersect the policy.
    pub(crate) fn flagged_selectors(&self, policy: VisibilityPolicy) -> BTreeSet<String> {
        if policy.strip_if_any.is_empty() {
            return BTreeSet::new();
        }
        self.flags
            .iter()
            .filter(|(_, f)| policy.strip_if_any.intersects(**f))
            .filter_map(|(id, _)| {
                let node = self.nodes.get(id)?;
                let tag = node.html_tag()?;
                let class = node.class_name();
                Some(make_selector(tag, class))
            })
            .collect()
    }
}

fn is_zero_size(r: &Rect) -> bool {
    (r.x1 - r.x0) < 1.0 && (r.y1 - r.y0) < 1.0
}

fn is_offscreen(r: &Rect) -> bool {
    r.x1 < 0.0 || r.y1 < 0.0 || r.x0 > OFFSCREEN_THRESHOLD_PX || r.y0 > OFFSCREEN_THRESHOLD_PX
}

/// Pure mapping from an AccessKit node to the visibility flags it implies.
fn compute_flags(node: &Node) -> VisibilityFlags {
    let mut f = VisibilityFlags::empty();
    if matches!(node.role(), Role::TabPanel) && node.is_selected() == Some(false) {
        f |= VisibilityFlags::TAB_PANEL_INACTIVE;
    }
    if let Some(size) = node.font_size()
        && size < 1.0
    {
        f |= VisibilityFlags::FONT_SIZE_ZERO;
    }
    if let Some(rect) = node.bounds() {
        if is_zero_size(&rect) {
            f |= VisibilityFlags::ZERO_SIZE;
        }
        if is_offscreen(&rect) {
            f |= VisibilityFlags::OFFSCREEN;
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_with_tag(role: Role) -> Node {
        let mut n = Node::new(role);
        n.set_html_tag("nav");
        n
    }

    #[test]
    fn flags_far_below_node_as_offscreen() {
        let mut nodes = HashMap::new();
        let mut n = node_with_tag(Role::GenericContainer);
        n.set_bounds(Rect {
            x0: 0.0,
            y0: 100_000.0,
            x1: 100.0,
            y1: 100_100.0,
        });
        nodes.insert(NodeId(1), n);
        let index = A11yIndex::new(&nodes);
        assert!(
            index
                .flags
                .get(&NodeId(1))
                .unwrap()
                .contains(VisibilityFlags::OFFSCREEN)
        );
    }

    #[test]
    fn flags_inactive_tab_panel() {
        let mut nodes = HashMap::new();
        let mut n = Node::new(Role::TabPanel);
        n.set_selected(false);
        nodes.insert(NodeId(1), n);
        let index = A11yIndex::new(&nodes);
        assert!(
            index
                .flags
                .get(&NodeId(1))
                .unwrap()
                .contains(VisibilityFlags::TAB_PANEL_INACTIVE)
        );
    }

    #[test]
    fn flags_zero_font_size() {
        let mut nodes = HashMap::new();
        let mut n = node_with_tag(Role::Paragraph);
        n.set_font_size(0.0);
        nodes.insert(NodeId(1), n);
        let index = A11yIndex::new(&nodes);
        assert!(
            index
                .flags
                .get(&NodeId(1))
                .unwrap()
                .contains(VisibilityFlags::FONT_SIZE_ZERO)
        );
    }

    #[test]
    fn flags_offscreen_left_of_viewport() {
        let mut nodes = HashMap::new();
        let mut n = node_with_tag(Role::GenericContainer);
        n.set_bounds(Rect {
            x0: -10_000.0,
            y0: 0.0,
            x1: -9_900.0,
            y1: 100.0,
        });
        nodes.insert(NodeId(1), n);
        let index = A11yIndex::new(&nodes);
        assert!(
            index
                .flags
                .get(&NodeId(1))
                .unwrap()
                .contains(VisibilityFlags::OFFSCREEN)
        );
    }

    #[test]
    fn boilerplate_selectors_are_sorted_and_unique() {
        let mut nodes = HashMap::new();
        let mut nav = node_with_tag(Role::Navigation);
        nav.set_class_name("primary");
        nodes.insert(NodeId(1), nav);
        let mut footer = Node::new(Role::ContentInfo);
        footer.set_html_tag("footer");
        nodes.insert(NodeId(2), footer);
        let mut nav2 = node_with_tag(Role::Navigation);
        nav2.set_class_name("primary");
        nodes.insert(NodeId(3), nav2);

        let index = A11yIndex::new(&nodes);
        let sels = index.boilerplate_selectors();
        assert_eq!(sels, BTreeSet::from(["footer".to_owned(), "nav.primary".to_owned()]));
    }

    #[test]
    fn search_role_strips_form_but_keeps_search_results_page() {
        let mut nodes = HashMap::new();
        let mut search_form = Node::new(Role::Search);
        search_form.set_html_tag("form");
        search_form.set_class_name("site-search");
        nodes.insert(NodeId(1), search_form);

        let mut search_main = Node::new(Role::Search);
        search_main.set_html_tag("main");
        nodes.insert(NodeId(2), search_main);

        let index = A11yIndex::new(&nodes);
        let sels = index.boilerplate_selectors();
        assert!(sels.contains("form.site-search"));
        assert!(!sels.iter().any(|s| s.starts_with("main")));
    }
}
