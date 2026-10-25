//! A small document tree that HTML and XML both load into, so one selector
//! engine reads either.

use scraper::Html;

#[derive(Debug, Clone)]
pub enum Kind {
    Document,
    Element {
        name: String,
        attrs: Vec<(String, String)>,
    },
    Text(String),
    Comment,
}

#[derive(Debug, Clone)]
struct Node {
    parent: Option<usize>,
    children: Vec<usize>,
    kind: Kind,
}

/// Nodes are numbered in the order they were added; 0 is the document.
#[derive(Debug, Clone)]
pub struct Dom {
    nodes: Vec<Node>,
}

pub type Id = usize;

impl Dom {
    fn new() -> Self {
        Self {
            nodes: vec![Node {
                parent: None,
                children: vec![],
                kind: Kind::Document,
            }],
        }
    }

    fn add(&mut self, parent: Id, kind: Kind) -> Id {
        let id = self.nodes.len();
        self.nodes.push(Node {
            parent: Some(parent),
            children: vec![],
            kind,
        });
        self.nodes[parent].children.push(id);
        id
    }

    pub fn parse_html(source: &str) -> Self {
        let html = Html::parse_document(source);
        let mut dom = Dom::new();
        fn walk(dom: &mut Dom, parent: Id, node: ego_tree_shim::NodeRef<'_>) {
            for child in node.children() {
                match child.value() {
                    scraper::Node::Element(e) => {
                        let id = dom.add(
                            parent,
                            Kind::Element {
                                name: e.name().to_ascii_lowercase(),
                                attrs: e
                                    .attrs()
                                    .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
                                    .collect(),
                            },
                        );
                        walk(dom, id, child);
                    }
                    scraper::Node::Text(t) => {
                        dom.add(parent, Kind::Text(t.to_string()));
                    }
                    scraper::Node::Comment(_) => {
                        dom.add(parent, Kind::Comment);
                    }
                    _ => walk(dom, parent, child),
                }
            }
        }
        walk(&mut dom, 0, html.tree.root());
        dom
    }

    pub fn parse_xml(source: &str) -> Result<Self, String> {
        // Feeds often start with a byte-order mark or stray whitespace.
        let source = source.trim_start_matches('\u{feff}').trim_start();
        let options = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        };
        let doc =
            roxmltree::Document::parse_with_options(source, options).map_err(|e| e.to_string())?;
        let mut dom = Dom::new();
        fn walk(dom: &mut Dom, parent: Id, node: roxmltree::Node<'_, '_>) {
            for child in node.children() {
                if child.is_element() {
                    let tag = child.tag_name();
                    // `torznab:attr` is reached by selectors as `torznab|attr`, but
                    // the definitions that read XML name it `attr`.
                    let name = tag.name().to_ascii_lowercase();
                    let id = dom.add(
                        parent,
                        Kind::Element {
                            name,
                            attrs: child
                                .attributes()
                                .map(|a| (a.name().to_ascii_lowercase(), a.value().to_owned()))
                                .collect(),
                        },
                    );
                    walk(dom, id, child);
                } else if child.is_text() {
                    dom.add(
                        parent,
                        Kind::Text(child.text().unwrap_or_default().to_owned()),
                    );
                }
            }
        }
        walk(&mut dom, 0, doc.root());
        Ok(dom)
    }

    pub fn root(&self) -> Id {
        0
    }

    pub fn kind(&self, id: Id) -> &Kind {
        &self.nodes[id].kind
    }

    pub fn parent(&self, id: Id) -> Option<Id> {
        self.nodes[id].parent
    }

    pub fn parent_element(&self, id: Id) -> Option<Id> {
        self.parent(id).filter(|p| self.is_element(*p))
    }

    pub fn is_element(&self, id: Id) -> bool {
        matches!(self.nodes[id].kind, Kind::Element { .. })
    }

    pub fn name(&self, id: Id) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn attr(&self, id: Id, name: &str) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn attrs(&self, id: Id) -> &[(String, String)] {
        match &self.nodes[id].kind {
            Kind::Element { attrs, .. } => attrs,
            _ => &[],
        }
    }

    pub fn children(&self, id: Id) -> &[Id] {
        &self.nodes[id].children
    }

    pub fn element_children(&self, id: Id) -> impl Iterator<Item = Id> + '_ {
        self.nodes[id]
            .children
            .iter()
            .copied()
            .filter(|c| self.is_element(*c))
    }

    /// The elements under `id`, nearest first in document order.
    pub fn descendants(&self, id: Id) -> Vec<Id> {
        let mut out = vec![];
        let mut stack: Vec<Id> = self.nodes[id].children.iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            if self.is_element(n) {
                out.push(n);
            }
            stack.extend(self.nodes[n].children.iter().rev().copied());
        }
        out
    }

    pub fn prev_element_sibling(&self, id: Id) -> Option<Id> {
        let parent = self.parent(id)?;
        let siblings = &self.nodes[parent].children;
        let at = siblings.iter().position(|s| *s == id)?;
        siblings[..at]
            .iter()
            .rev()
            .copied()
            .find(|s| self.is_element(*s))
    }

    pub fn text_content(&self, id: Id) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: Id, out: &mut String) {
        match &self.nodes[id].kind {
            Kind::Text(t) => out.push_str(t),
            Kind::Comment => {}
            _ => {
                for c in &self.nodes[id].children {
                    self.collect_text(*c, out);
                }
            }
        }
    }

    /// Takes an element (and what is under it) out of the document.
    pub fn remove(&mut self, id: Id) {
        if let Some(parent) = self.nodes[id].parent.take() {
            self.nodes[parent].children.retain(|c| *c != id);
        }
    }

    /// Moves the children of `from` to the end of `into`'s children.
    pub fn append_children_of(&mut self, into: Id, from: Id) {
        let moved = std::mem::take(&mut self.nodes[from].children);
        for c in &moved {
            self.nodes[*c].parent = Some(into);
        }
        self.nodes[into].children.extend(moved);
    }

    /// The element's HTML, for messages only.
    pub fn outer_html(&self, id: Id) -> String {
        let mut out = String::new();
        self.write_html(id, &mut out);
        out
    }

    fn write_html(&self, id: Id, out: &mut String) {
        match &self.nodes[id].kind {
            Kind::Text(t) => out.push_str(t),
            Kind::Element { name, attrs } => {
                out.push('<');
                out.push_str(name);
                for (k, v) in attrs {
                    out.push_str(&format!(" {k}=\"{v}\""));
                }
                out.push('>');
                for c in &self.nodes[id].children {
                    self.write_html(*c, out);
                }
                out.push_str(&format!("</{name}>"));
            }
            _ => {
                for c in &self.nodes[id].children {
                    self.write_html(*c, out);
                }
            }
        }
    }
}

/// `scraper` re-exports its tree type under a path that is awkward to name.
mod ego_tree_shim {
    pub type NodeRef<'a> = ego_tree::NodeRef<'a, scraper::Node>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_and_xml_load_into_the_same_shape() {
        let h = Dom::parse_html("<table><tr><td class=a>One</td><td>Two</td></tr></table>");
        let tds: Vec<_> = h
            .descendants(0)
            .into_iter()
            .filter(|n| h.name(*n) == Some("td"))
            .collect();
        assert_eq!(tds.len(), 2);
        assert_eq!(h.attr(tds[0], "class"), Some("a"));
        assert_eq!(h.text_content(tds[1]), "Two");

        let x = Dom::parse_xml("<?xml version=\"1.0\"?><rss><item><title>T</title></item></rss>")
            .unwrap();
        let t = x
            .descendants(0)
            .into_iter()
            .find(|n| x.name(*n) == Some("title"))
            .unwrap();
        assert_eq!(x.text_content(t), "T");
    }

    #[test]
    fn removing_and_merging_rows_works() {
        let mut d = Dom::parse_html("<ul><li id=a>A</li><li id=b>B</li></ul>");
        let lis: Vec<_> = d
            .descendants(0)
            .into_iter()
            .filter(|n| d.name(*n) == Some("li"))
            .collect();
        d.append_children_of(lis[0], lis[1]);
        assert_eq!(d.text_content(lis[0]), "AB");
        d.remove(lis[1]);
        assert_eq!(
            d.descendants(0)
                .into_iter()
                .filter(|n| d.name(*n) == Some("li"))
                .count(),
            1
        );
    }
}
