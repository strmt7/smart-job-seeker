//! T033 — manual fallback packet.
//!
//! When a page is unsupported, the prepared packet is preserved and turned
//! into copy-ready material for a manual handoff. The app never improvises
//! unbounded computer-use on an unknown form.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackItem {
    pub label: String,
    pub value: String,
    pub multiline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackPacket {
    pub application_id: String,
    pub job_title: String,
    pub employer: String,
    pub form_url: String,
    pub items: Vec<FallbackItem>,
}

impl FallbackPacket {
    /// Render copy-ready blocks. Deterministic; safe for clipboard.
    pub fn to_clipboard_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Application to {} — {}\n",
            self.employer, self.job_title
        ));
        out.push_str(&format!("Form: {}\n\n", self.form_url));
        for item in &self.items {
            if item.multiline {
                out.push_str(&format!("=== {} ===\n{}\n\n", item.label, item.value));
            } else {
                out.push_str(&format!("{}: {}\n", item.label, item.value));
            }
        }
        out
    }

    /// Structured per-field copy targets for the UI's copy buttons.
    pub fn copy_targets(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .map(|i| (i.label.clone(), i.value.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_page_yields_copy_ready_handoff() {
        let p = FallbackPacket {
            application_id: "app-7".into(),
            job_title: "Rust Engineer".into(),
            employer: "Acme".into(),
            form_url: "https://careers.acme.com/apply/7".into(),
            items: vec![
                FallbackItem {
                    label: "Full name".into(),
                    value: "Ada Lovelace".into(),
                    multiline: false,
                },
                FallbackItem {
                    label: "Cover letter".into(),
                    value: "Dear team,\n...".into(),
                    multiline: true,
                },
            ],
        };
        let text = p.to_clipboard_text();
        assert!(text.contains("Application to Acme — Rust Engineer"));
        assert!(text.contains("=== Cover letter ==="));
        assert_eq!(p.copy_targets().len(), 2);
        // deterministic
        assert_eq!(text, p.to_clipboard_text());
    }
}
