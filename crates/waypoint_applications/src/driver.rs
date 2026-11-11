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
    /// Completion barriers and unresolved controls. Every item still blocks
    /// advancement; page-wide barriers also prohibit independent field entry.
    pub walls: Vec<String>,
    /// Snapshot-bound eligibility for independent, supported field entry.
    /// This never authorizes navigation, submission or an external write.
    pub independent_fields: Option<IndependentFieldAccess>,
}

/// A visible custom widget that remains unresolved. Its presence must continue
/// to block completion, but need not prevent entering a separate known field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomControlReview {
    pub label: Option<String>,
    pub required: bool,
}

impl CustomControlReview {
    fn message(&self) -> String {
        let prefix = if self.required {
            "a required custom control needs manual review"
        } else {
            "a custom control needs manual review"
        };
        match &self.label {
            Some(label) => format!("{prefix}: {label}"),
            None => prefix.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionReview {
    CustomControl(CustomControlReview),
    HiddenRequiredControl,
    ChallengeSupportReference,
}

impl CompletionReview {
    fn message(&self) -> String {
        match self {
            Self::CustomControl(control) => control.message(),
            Self::HiddenRequiredControl => "a required hidden control needs manual review".into(),
            Self::ChallengeSupportReference => {
                "CAPTCHA support is referenced; completion needs review".into()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndependentFieldAccess {
    schema_identity: Option<String>,
    inspected_walls: Vec<String>,
    page_barriers: Vec<String>,
}

impl LiveForm {
    /// Called by a complete browser inspection. Page-wide barriers and custom
    /// widget reviews remain separate until their human-readable presentation.
    pub fn from_inspection(
        schema: FormSchema,
        page_barriers: Vec<String>,
        completion_reviews: Vec<CompletionReview>,
    ) -> Self {
        let mut walls = page_barriers.clone();
        walls.extend(completion_reviews.iter().map(CompletionReview::message));
        walls.sort();
        walls.dedup();
        let evidence = IndependentFieldAccess {
            schema_identity: Some(schema.schema_hash_input()),
            inspected_walls: walls.clone(),
            page_barriers,
        };
        Self {
            origin: schema.origin.clone(),
            schema,
            walls,
            independent_fields: Some(evidence),
        }
    }

    /// Namespace native control identities without inventing a new inspection.
    /// Only an unchanged inspection can retain its evidence through this exact
    /// selector/section-ID transformation. Existing barriers remain barriers.
    pub fn with_control_scope(mut self, prefix: &str) -> Self {
        let current = self.independent_fields.as_ref().is_some_and(|evidence| {
            evidence.inspected_walls == self.walls
                && evidence.schema_identity.as_ref() == Some(&self.schema.schema_hash_input())
                && self.origin == self.schema.origin
        });
        for field in &mut self.schema.fields {
            field.selector = format!("{prefix}{}", field.selector);
            for section in &mut field.context.sections {
                section.id = format!("{prefix}{}", section.id);
            }
        }
        let identity = current.then(|| self.schema.schema_hash_input());
        if let Some(evidence) = self.independent_fields.as_mut() {
            // None is a persistent invalidation: a later prefix cannot repair
            // previously stale evidence by coincidentally restoring old bytes.
            evidence.schema_identity = identity;
        }
        self
    }

    /// Unknown/legacy walls still block. Any later change to the schema, origin
    /// or review observations invalidates the inspection's entry eligibility.
    pub fn independent_field_entry_blocked(&self) -> bool {
        match &self.independent_fields {
            Some(evidence) => {
                !evidence.page_barriers.is_empty()
                    || evidence.inspected_walls != self.walls
                    || evidence.schema_identity.as_ref() != Some(&self.schema.schema_hash_input())
                    || self.origin != self.schema.origin
            }
            None => !self.walls.is_empty(),
        }
    }

    /// Explain a stop without treating a model's opinion as inspection evidence.
    pub fn independent_field_barriers(&self) -> Vec<String> {
        match &self.independent_fields {
            Some(evidence)
                if evidence.inspected_walls == self.walls
                    && evidence.schema_identity.as_ref()
                        == Some(&self.schema.schema_hash_input())
                    && self.origin == self.schema.origin =>
            {
                evidence.page_barriers.clone()
            }
            Some(_) => vec![
                "The form changed after inspection; inspect it again before entering fields."
                    .into(),
            ],
            None => self.walls.clone(),
        }
    }
}

#[cfg(test)]
mod independent_field_tests {
    use super::*;

    fn schema() -> FormSchema {
        FormSchema {
            origin: "https://example.org".into(),
            fields: vec![],
            multi_step: false,
            steps: 1,
        }
    }

    fn custom(label: &str) -> CompletionReview {
        CompletionReview::CustomControl(CustomControlReview {
            label: Some(label.into()),
            required: true,
        })
    }

    #[test]
    fn custom_review_permits_independent_entry_but_remains_a_completion_barrier() {
        let form = LiveForm::from_inspection(schema(), vec![], vec![custom("Country")]);
        assert!(!form.independent_field_entry_blocked());
        assert_eq!(
            form.walls,
            ["a required custom control needs manual review: Country"]
        );
        let mut legacy = form.clone();
        legacy.independent_fields = None;
        assert!(legacy.independent_field_entry_blocked());
    }

    #[test]
    fn hard_barrier_cannot_be_downgraded_by_a_matching_custom_control_message() {
        let review = custom("Country");
        let form = LiveForm::from_inspection(schema(), vec![review.message()], vec![review]);
        assert_eq!(form.walls.len(), 1);
        assert!(form.independent_field_entry_blocked());
        let captcha = LiveForm::from_inspection(
            schema(),
            vec!["captcha present".into()],
            vec![custom("captcha present")],
        );
        assert!(captcha.independent_field_entry_blocked());
    }

    #[test]
    fn changed_or_removed_review_and_schema_origin_invalidate_entry_evidence() {
        let original = LiveForm::from_inspection(schema(), vec![], vec![custom("Country")]);
        let mut added = original.clone();
        added.walls.push("new inspection uncertainty".into());
        assert!(added.independent_field_entry_blocked());
        let mut removed = original.clone();
        removed.walls.clear();
        assert!(removed.independent_field_entry_blocked());
        let mut changed = original.clone();
        changed.schema.steps = 2;
        assert!(changed.independent_field_entry_blocked());
        let mut origin = original;
        origin.origin = "https://other.example.org".into();
        assert!(origin.independent_field_entry_blocked());
    }

    fn field_schema(selector: &str) -> FormSchema {
        let mut schema = schema();
        schema.fields.push(serde_json::from_value(serde_json::json!({
            "selector":selector,"label":"Name","field_type":"text","required":true,"sensitive":false,
            "context":{"sections":[{"id":"history","label":"History","inferred":false}]}
        })).unwrap());
        schema
    }

    #[test]
    fn native_scope_preserves_current_evidence_and_completion_reviews() {
        let form =
            LiveForm::from_inspection(field_schema("#name"), vec![], vec![custom("Country")]);
        let walls = form.walls.clone();
        let scoped = form.with_control_scope("captured:123:");
        assert_eq!(scoped.schema.fields[0].selector, "captured:123:#name");
        assert_eq!(
            scoped.schema.fields[0].context.sections[0].id,
            "captured:123:history"
        );
        assert_eq!(scoped.walls, walls);
        assert!(!scoped.independent_field_entry_blocked());
        assert!(scoped.independent_field_barriers().is_empty());
        let mut changed = scoped;
        changed.schema.fields[0].required = false;
        assert!(changed.independent_field_entry_blocked());
    }

    #[test]
    fn scoping_never_refreshes_barriers_stale_snapshots_or_legacy_evidence() {
        let blocked = LiveForm::from_inspection(
            field_schema("#name"),
            vec!["captcha present".into()],
            vec![],
        )
        .with_control_scope("captured:123:");
        assert!(blocked.independent_field_entry_blocked());
        assert_eq!(blocked.independent_field_barriers(), ["captcha present"]);
        let mut original_schema = field_schema("captured:#name");
        original_schema.fields[0].context.sections[0].id = "captured:history".into();
        let original = LiveForm::from_inspection(original_schema, vec![], vec![]);
        let mut stale = original.clone();
        stale.schema.fields[0].selector = "#name".into();
        stale.schema.fields[0].context.sections[0].id = "history".into();
        let stale = stale.with_control_scope("captured:");
        assert!(stale.independent_field_entry_blocked());
        assert!(stale
            .with_control_scope("")
            .independent_field_entry_blocked());
        let mut walls = original.clone();
        walls.walls.push("unverified".into());
        assert!(walls
            .with_control_scope("x:")
            .independent_field_entry_blocked());
        let mut origin = original.clone();
        origin.origin = "https://other.example.org".into();
        assert!(origin
            .with_control_scope("x:")
            .independent_field_entry_blocked());
        let mut legacy = original;
        legacy.independent_fields = None;
        assert!(legacy.with_control_scope("x:").independent_fields.is_none());
    }

    #[test]
    fn unknown_legacy_wall_is_never_treated_as_a_custom_control_advisory() {
        let form = LiveForm {
            origin: schema().origin.clone(),
            schema: schema(),
            walls: vec!["a custom control needs manual review: Country".into()],
            independent_fields: None,
        };
        assert!(form.independent_field_entry_blocked());
    }

    #[test]
    fn passive_challenge_reference_and_hidden_required_control_still_block_completion() {
        let form = LiveForm::from_inspection(
            schema(),
            vec![],
            vec![
                CompletionReview::ChallengeSupportReference,
                CompletionReview::HiddenRequiredControl,
            ],
        );
        assert!(!form.independent_field_entry_blocked());
        assert_eq!(form.walls.len(), 2);
        assert!(form
            .walls
            .iter()
            .any(|wall| wall.contains("CAPTCHA support")));
    }
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PageLink {
    pub url: String,
    pub label: String,
}

/// A full inventory at this bound may omit additional links.
pub const PAGE_LINK_LIMIT: usize = 200;

/// The bounded current selection shown to the user before a replacement.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SelectSnapshot {
    pub index: usize,
    pub label: String,
    pub value: String,
}

/// The bounded current text shown locally before an explicit replacement.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TextSnapshot {
    pub value: String,
}

/// Bounded state of a checkbox, radio group or multiple-selection control.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChoiceSnapshot {
    pub kind: String,
    pub checked: Option<bool>,
    pub selections: Vec<SelectSnapshot>,
}

pub trait BrowserDriver {
    /// Open the destination and describe the live form.
    fn open(&mut self, url: &str) -> Result<LiveForm, DriverError>;
    /// Inspect the current page without navigating or resetting the form.
    fn inspect_current(&mut self) -> Result<LiveForm, DriverError> {
        Err(DriverError(
            "Current-page inspection is not supported by this driver".into(),
        ))
    }
    /// Return the currently loaded URL when supported by the driver.
    fn current_url(&mut self) -> Result<String, DriverError> {
        Err(DriverError("Current URL is unavailable".into()))
    }
    /// Read application-looking links without clicking them.
    fn application_links(&mut self) -> Result<Vec<String>, DriverError> {
        Err(DriverError("Application links are unavailable".into()))
    }
    /// Return a bounded list of page anchor destinations for explicit user review.
    fn page_links(&mut self) -> Result<Vec<PageLink>, DriverError> {
        Err(DriverError("Page links are unavailable".into()))
    }
    /// Verify a text-like field after other form commands have run.
    fn verify_text_value(&mut self, _selector: &str, _expected: &str) -> Result<(), DriverError> {
        Err(DriverError("Post-fill verification is unavailable".into()))
    }
    /// Check a numeric candidate without changing the live field.
    fn check_numeric_candidate(
        &mut self,
        _selector: &str,
        _value: &str,
    ) -> Result<(), DriverError> {
        Err(DriverError("Numeric preflight is unavailable".into()))
    }
    /// Check a text candidate without changing the live field.
    fn check_text_candidate(&mut self, _selector: &str, _value: &str) -> Result<(), DriverError> {
        Err(DriverError("Text preflight is unavailable".into()))
    }
    /// Check a date or time candidate without changing the live field.
    fn check_temporal_candidate(
        &mut self,
        _selector: &str,
        _value: &str,
    ) -> Result<(), DriverError> {
        Err(DriverError("Date and time preflight is unavailable".into()))
    }
    /// Refuse to replace an existing, different text value without manual review.
    fn ensure_text_replaceable(
        &mut self,
        _selector: &str,
        _expected: &str,
    ) -> Result<(), DriverError> {
        Err(DriverError(
            "Existing field value cannot be inspected".into(),
        ))
    }
    /// Refuse to replace a different existing choice without manual review.
    fn ensure_choice_replaceable(&mut self, _command: &BrowserCommand) -> Result<(), DriverError> {
        Err(DriverError("Existing choice cannot be inspected".into()))
    }
    fn selected_option_snapshot(&mut self, _selector: &str) -> Result<SelectSnapshot, DriverError> {
        Err(DriverError("Existing selection cannot be inspected".into()))
    }
    fn text_value_snapshot(&mut self, _selector: &str) -> Result<TextSnapshot, DriverError> {
        Err(DriverError("Existing text cannot be inspected".into()))
    }
    fn choice_snapshot(
        &mut self,
        _selector: &str,
        _kind: &str,
    ) -> Result<ChoiceSnapshot, DriverError> {
        Err(DriverError("Existing choices cannot be inspected".into()))
    }
    /// Verify a supported choice without changing it.
    fn verify_choice(&mut self, _command: &BrowserCommand) -> Result<(), DriverError> {
        Err(DriverError("Choice verification is unavailable".into()))
    }
    /// Optional, partial count of observed non-GET/HEAD requests. No count can
    /// establish that the page has not sent data by another route.
    fn observed_non_read_requests(&mut self) -> Result<Option<u64>, DriverError> {
        Ok(None)
    }
    /// Execute exactly one typed command. No scripting surface exists.
    fn execute(&mut self, command: &BrowserCommand) -> Result<(), DriverError>;
    /// Attempt submission. Earlier page interactions can also send data or change remote state.
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
        "legally eligible to work",
        "legally allowed to work",
        "legally permitted to work",
        "gender",
        "ethnic",
        "veteran",
        "disability",
        "date of birth",
        "social security",
        "national id",
        "passport",
        "nationality",
        "citizenship",
        "consent",
        "i agree",
        "terms",
        "privacy",
        "gdpr",
        "declaration",
        // These are review triggers, not a form-field mapping or a complete
        // multilingual classifier. Unknown languages still need user review.
        "arbeitserlaubnis",
        "arbeitsgenehmigung",
        "aufenthaltserlaubnis",
        "gehalt",
        "geschlecht",
        "behinderung",
        "geburtsdatum",
        "einwilligung",
        "datenschutz",
        "staatsangehörigkeit",
        "permiso de trabajo",
        "autorización de trabajo",
        "autorizacion de trabajo",
        "patrocinio",
        "salario",
        "sueldo",
        "género",
        "genero",
        "discapacidad",
        "fecha de nacimiento",
        "consentimiento",
        "privacidad",
        "nacionalidad",
        "autorisation de travail",
        "permis de travail",
        "salaire",
        "rémunération",
        "remuneration",
        "handicap",
        "sexe",
        "date de naissance",
        "consentement",
        "confidentialité",
        "nationalité",
    ];
    MARKERS.iter().any(|m| l.contains(m))
        || l.split(|c: char| !c.is_alphanumeric())
            .any(|word| matches!(word, "race" | "dob" | "ssn" | "age"))
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
    let mut end = text.len().min(CONFIRMATION_SCAN_LIMIT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = end < text.len();
    let text = &text[..end];
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
            if let Some(found) = identifier_in(&text[start..], STOPWORDS, truncated) {
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
fn identifier_in(tail: &str, stopwords: &[&str], truncated: bool) -> Option<String> {
    for token in tail.split_whitespace().take(4) {
        // A token reaching the scan boundary may be only part of an ID.
        if truncated && token.as_bytes().as_ptr_range().end == tail.as_bytes().as_ptr_range().end {
            continue;
        }
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
            "Are you legally eligible to work here?",
            "Besitzen Sie eine Arbeitserlaubnis?",
            "Geschlecht",
            "Gehaltsvorstellung",
            "¿Necesita patrocinio laboral?",
            "Fecha de nacimiento",
            "Autorisation de travail",
            "Rémunération souhaitée",
            "Datenschutzerklärung",
            "Política de privacidad",
            "Nationalité",
            "Race / ethnicity",
            "Passport Country",
            "Nationality",
            "Citizenship status",
            "Age",
            "What is your age?",
        ] {
            assert!(is_sensitive_label(label), "{label} must be sensitive");
        }
        for label in [
            "Cover letter",
            "Resume",
            "First name",
            "Email address",
            "Grace",
            "Manage projects",
            "Vorname",
            "E-Mail-Adresse",
            "Nombre",
            "Correo electrónico",
            "Prénom",
        ] {
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

    #[test]
    fn confirmation_limit_does_not_split_utf8_or_accept_partial_identifiers() {
        let multibyte = format!(
            "{}éApplication ID: ABC123",
            "x".repeat(CONFIRMATION_SCAN_LIMIT - 1)
        );
        assert_eq!(extract_application_ref(&multibyte), None);
        let truncated_id = "Application ID: ABC123";
        let partial = format!(
            "{}{truncated_id}EXTRA",
            " ".repeat(CONFIRMATION_SCAN_LIMIT - truncated_id.len())
        );
        assert_eq!(extract_application_ref(&partial), None);
        let complete_id = "Application ID: ABC123 ";
        let complete = format!(
            "{}{complete_id}more",
            " ".repeat(CONFIRMATION_SCAN_LIMIT - complete_id.len())
        );
        assert_eq!(extract_application_ref(&complete), Some("ABC123".into()));
    }
}
