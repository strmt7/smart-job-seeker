//! The browser boundary contract.
//!
//! This lives beside the form model because it *is* the form model's other
//! half: the schema describes a page, and this describes who may act on it.
//! Implementations: `waypoint_browser::CdpDriver` (a real browser over CDP)
//! and scripted drivers in tests.
//!
//! Deliberately narrow — open, execute one typed command, commit. There is no
//! "run script" and no way to set a field except through `BrowserCommand`.

use crate::form_mapping::{BrowserCommand, FormSchema};
use crate::receipt::ReceiptKind;

/// What the live page looks like right now.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveForm {
    pub origin: String,
    pub schema: FormSchema,
    /// Hard blockers discovered on the page ("captcha", "login required", ...).
    /// A wall is never something the app tries to climb.
    pub walls: Vec<String>,
}

/// What the site said after the write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteAck {
    pub receipt_kind: ReceiptKind,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverError(pub String);

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DriverError {}

pub trait BrowserDriver {
    /// Open the destination and describe the live form.
    fn open(&mut self, url: &str) -> Result<LiveForm, DriverError>;
    /// Execute exactly one typed command. No scripting surface exists.
    fn execute(&mut self, command: &BrowserCommand) -> Result<(), DriverError>;
    /// The single external write. Everything before this is reversible.
    fn commit(&mut self) -> Result<SiteAck, DriverError>;
}

/// Field labels that must never be answered on the candidate's behalf.
/// Conservative on purpose: a false positive only asks the candidate to type
/// one more answer; a false negative would put words in their mouth.
pub fn is_sensitive_label(label: &str) -> bool {
    let l = label.to_lowercase();
    const MARKERS: &[&str] = &[
        "salary",
        "compensation",
        "expectation",
        "desired pay",
        "authorization",
        "authorisation",
        "sponsor",
        "visa",
        "work permit",
        "gender",
        "race",
        "ethnic",
        "veteran",
        "disability",
        "date of birth",
        "dob",
        "social security",
        "ssn",
        "national id",
        "consent",
        "i agree",
        "terms",
        "privacy",
        "gdpr",
        "declaration",
    ];
    MARKERS.iter().any(|m| l.contains(m))
}

/// How many bytes of a page's visible text were searched for a confirmation.
pub const CONFIRMATION_SCAN_LIMIT: usize = 40_000;

/// Extract a stable application identifier from a confirmation page's text.
///
/// Only patterns that a site would not emit accidentally count. A generic
/// "thanks, we received it" paragraph deliberately does not.
///
/// Case matters: an application reference is an identifier the candidate may
/// have to quote back to the employer, so the *original* casing is returned.
/// Matching uses an ASCII-only lowercase copy, whose byte offsets are
/// identical to the original for every input (Unicode-aware lowercasing can
/// change byte lengths and would mis-slice).
pub fn extract_application_ref(text: &str) -> Option<String> {
    let text = &text[..text.len().min(CONFIRMATION_SCAN_LIMIT)];
    const KEYS: &[&str] = &[
        "application id",
        "application reference",
        "application number",
        "reference number",
        "confirmation number",
        "confirmation id",
        "candidate id",
        "reference:",
        "confirmation:",
        "ref:",
        "id:",
    ];
    // Words that sit between the key and the identifier ("your reference
    // number IS 88-2201") and are never the identifier themselves.
    const STOPWORDS: &[&str] = &[
        "is",
        "no",
        "number",
        "ref",
        "reference",
        "id",
        "application",
        "confirmation",
        "the",
        "your",
        "our",
    ];
    let lowered = text.to_ascii_lowercase();
    for key in KEYS {
        let mut from = 0usize;
        while let Some(pos) = lowered[from..].find(key) {
            let start = from + pos + key.len();
            if let Some(found) = identifier_in(&text[start..], STOPWORDS) {
                return Some(found);
            }
            from = start;
            if from >= lowered.len() {
                break;
            }
        }
    }
    None
}

/// The first token in `tail` that could plausibly be an identifier: at least
/// three characters, containing a digit (which is what makes it a reference
/// rather than a word), and not a connector word.
fn identifier_in(tail: &str, stopwords: &[&str]) -> Option<String> {
    for token in tail.split_whitespace().take(4) {
        let cleaned: String = token
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if cleaned.len() < 3 {
            continue;
        }
        if !cleaned.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        if stopwords.iter().any(|s| cleaned.eq_ignore_ascii_case(s)) {
            continue;
        }
        // Guard against swallowing the *next* field label, e.g.
        // "Application ID: (none) Candidate ID: X1".
        if cleaned.chars().all(|c| c.is_ascii_digit()) && cleaned.len() > 12 {
            continue;
        }
        return Some(cleaned);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_markers_cover_the_fields_that_matter() {
        for label in [
            "Salary expectation",
            "Work authorization",
            "Do you require visa sponsorship?",
            "Gender",
            "Veteran status",
            "I agree to the privacy policy",
            "Consent to process my data",
        ] {
            assert!(is_sensitive_label(label), "{label} must be sensitive");
        }
        for label in ["Cover letter", "Resume", "First name", "Email address"] {
            assert!(!is_sensitive_label(label), "{label} must not be sensitive");
        }
    }

    #[test]
    fn application_refs_are_extracted_from_realistic_confirmations() {
        assert_eq!(
            extract_application_ref("Application received. Application ID: GH-44117 Thank you."),
            Some("GH-44117".into())
        );
        assert_eq!(
            extract_application_ref("Your reference number is 88-2201."),
            Some("88-2201".into())
        );
        assert_eq!(
            extract_application_ref("Confirmation: ABCD1234"),
            Some("ABCD1234".into())
        );
    }

    #[test]
    fn an_application_reference_keeps_its_original_casing() {
        // The candidate may have to quote this back to the employer, so the
        // identifier must come back exactly as the site printed it.
        assert_eq!(
            extract_application_ref("Application ID: GH-44117"),
            Some("GH-44117".into())
        );
        assert_eq!(
            extract_application_ref("application id: gh-44117"),
            Some("gh-44117".into())
        );
        assert_eq!(
            extract_application_ref("Ref: AbC-90210x"),
            Some("AbC-90210x".into())
        );
        // Multibyte text before the key must not shift the extraction.
        assert_eq!(
            extract_application_ref("Grüße aus Zürich — Application ID: XY-99"),
            Some("XY-99".into())
        );
    }

    #[test]
    fn a_generic_thank_you_is_not_a_confirmation() {
        assert_eq!(
            extract_application_ref(
                "Thanks for applying! We received your application and will be in touch."
            ),
            None
        );
        assert_eq!(extract_application_ref(""), None);
        // Key present but no identifier: still not a confirmation.
        assert_eq!(extract_application_ref("Application ID: "), None);
        assert_eq!(extract_application_ref("Application id: —"), None);
    }
}
