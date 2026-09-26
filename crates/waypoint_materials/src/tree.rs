//! T021 — canonical semantic document tree.
//!
//! The LLM proposes changes to this tree, never to OOXML/PDF bytes. Locked
//! spans (exact employer names, dates, titles) are preserved by deterministic
//! constraint code, not by prompt.

use serde::{Deserialize, Serialize};
use waypoint_domain::ClaimId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocNodeKind {
    Document,
    Heading,
    Role,
    Bullet,
    Paragraph,
    TextSpan,
    Link,
    ImageRef,
}

/// A node's text may cite claims. `locked` spans are user-frozen wording the
/// generator must not touch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocNode {
    pub id: String,
    pub kind: DocNodeKind,
    pub text: String,
    pub claim_ids: Vec<ClaimId>,
    pub locked: bool,
    pub children: Vec<DocNode>,
}

impl DocNode {
    pub fn text_span(text: impl Into<String>, claim: Option<ClaimId>) -> Self {
        Self {
            id: format!("n{}", next_id()),
            kind: DocNodeKind::TextSpan,
            text: text.into(),
            claim_ids: claim.into_iter().collect(),
            locked: false,
            children: vec![],
        }
    }

    pub fn heading(text: impl Into<String>) -> Self {
        Self {
            id: format!("n{}", next_id()),
            kind: DocNodeKind::Heading,
            text: text.into(),
            claim_ids: vec![],
            locked: false,
            children: vec![],
        }
    }

    /// Collect (node_id, text) of every unlocked text span citing `claim`.
    pub fn spans_citing(&self, claim: &ClaimId) -> Vec<(String, String)> {
        let mut out = vec![];
        if self.claim_ids.contains(claim) && !self.locked {
            out.push((self.id.clone(), self.text.clone()));
        }
        for c in &self.children {
            out.extend(c.spans_citing(claim));
        }
        out
    }

    /// Apply a replacement to nodes citing `claim` unless locked; returns
    /// the node ids actually changed.
    pub fn replace_in_claim_spans(&mut self, claim: &ClaimId, new_text: &str) -> Vec<String> {
        let mut changed = vec![];
        if self.claim_ids.contains(claim) && !self.locked && self.text != new_text {
            self.text = new_text.to_string();
            changed.push(self.id.clone());
        }
        for c in &mut self.children {
            changed.extend(c.replace_in_claim_spans(claim, new_text));
        }
        changed
    }
}

use std::sync::atomic::{AtomicU64, Ordering};
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// A named artifact (CV, cover letter, answer) rendered from the tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticDocument {
    pub id: String,
    pub kind: String, // "cv" | "cover_letter" | "answer"
    pub root: DocNode,
    /// Profile revision this document was generated from.
    pub profile_revision: u64,
}

impl SemanticDocument {
    pub fn spans_citing(&self, claim: &ClaimId) -> Vec<(String, String)> {
        self.root.spans_citing(claim)
    }

    /// Deterministic flatten to plain text (renderers do formatting, not us).
    pub fn to_plain_text(&self) -> String {
        fn walk(n: &DocNode, out: &mut String) {
            match n.kind {
                DocNodeKind::Bullet => out.push_str("- "),
                DocNodeKind::Heading => out.push_str("\n## "),
                _ => {}
            }
            if !n.text.is_empty() {
                out.push_str(&n.text);
            }
            if n.kind == DocNodeKind::Heading || !n.children.is_empty() {
                out.push('\n');
            }
            for c in &n.children {
                walk(c, out);
            }
        }
        let mut s = String::new();
        walk(&self.root, &mut s);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_citing_finds_unlocked_claim_text() {
        let claim = ClaimId::new("c1").unwrap();
        let mut doc = SemanticDocument {
            id: "cv1".into(),
            kind: "cv".into(),
            profile_revision: 3,
            root: DocNode {
                id: "root".into(),
                kind: DocNodeKind::Document,
                text: String::new(),
                claim_ids: vec![],
                locked: false,
                children: vec![
                    DocNode::heading("Experience"),
                    DocNode::text_span("Led 2019-2021", Some(claim.clone())),
                    DocNode {
                        id: "locked-span".into(),
                        kind: DocNodeKind::TextSpan,
                        text: "Led 2019-2021 (locked)".into(),
                        claim_ids: vec![claim.clone()],
                        locked: true,
                        children: vec![],
                    },
                ],
            },
        };

        let spans = doc.spans_citing(&claim);
        assert_eq!(spans.len(), 1, "locked span must be excluded");
        assert_eq!(spans[0].1, "Led 2019-2021");

        let changed = doc.root.replace_in_claim_spans(&claim, "Led 2020-2022");
        assert_eq!(changed.len(), 1, "only the unlocked citing span changes");
        assert_eq!(
            doc.spans_citing(&claim)[0].1,
            "Led 2020-2022",
            "unlocked span text updated"
        );
        // locked node untouched
        let locked = doc
            .root
            .children
            .iter()
            .find(|c| c.id == "locked-span")
            .unwrap();
        assert_eq!(locked.text, "Led 2019-2021 (locked)");
    }

    #[test]
    fn plain_text_flatten_is_deterministic() {
        let doc = SemanticDocument {
            id: "x".into(),
            kind: "cv".into(),
            profile_revision: 1,
            root: DocNode {
                id: "r".into(),
                kind: DocNodeKind::Document,
                text: String::new(),
                claim_ids: vec![],
                locked: false,
                children: vec![DocNode::heading("Skills"), DocNode::text_span("Rust", None)],
            },
        };
        let a = doc.to_plain_text();
        let b = doc.to_plain_text();
        assert_eq!(a, b);
        assert!(a.contains("## Skills"));
        assert!(a.contains("Rust"));
    }
}
