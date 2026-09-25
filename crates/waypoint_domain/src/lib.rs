//! Waypoint domain contracts (task T004, gate G0).
//!
//! Typed identities, revisions, states, grants, observations and amount units.
//! Acceptance: schemas must reject unknown state, missing source, invalid
//! currency basis and unbound approval.

use serde::{Deserialize, Serialize};

/// A typed, immutable identifier for an entity kind.
macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Result<Self, DomainError> {
                let s = s.into();
                if s.trim().is_empty() {
                    return Err(DomainError::EmptyId(stringify!($name)));
                }
                Ok(Self(s))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

id_type!(EvidenceId);
id_type!(ClaimId);
id_type!(JobIdentityId);
id_type!(IntentId);
id_type!(GrantId);
id_type!(ProfileId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    EmptyId(&'static str),
    InvalidSha256(String),
    InvalidCurrencyBasis,
    UnsupportedClaimWithoutSource,
    UnboundApproval,
    InvalidState(String),
    ApprovalNotBoundToPayload,
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomainError::EmptyId(k) => write!(f, "empty identifier for {k}"),
            DomainError::InvalidSha256(s) => write!(f, "invalid sha256 hex digest: {s}"),
            DomainError::InvalidCurrencyBasis => {
                write!(
                    f,
                    "amount must pair currency with a gross/net basis and time period"
                )
            }
            DomainError::UnsupportedClaimWithoutSource => {
                write!(
                    f,
                    "externally supported claims require external confirmation evidence"
                )
            }
            DomainError::UnboundApproval => write!(f, "approval grant is not bound to an intent"),
            DomainError::InvalidState(s) => write!(f, "unknown application state: {s}"),
            DomainError::ApprovalNotBoundToPayload => {
                write!(
                    f,
                    "approval grant payload digest does not match intent artifacts"
                )
            }
        }
    }
}

impl std::error::Error for DomainError {}

/// Lowercase hex SHA-256 digest wrapper; construction validates the format.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn new(s: impl Into<String>) -> Result<Self, DomainError> {
        let s = s.into();
        if s.len() == 64
            && s.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            Ok(Self(s))
        } else {
            Err(DomainError::InvalidSha256(s))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for Sha256Digest {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Sha256Digest::new(s).map_err(serde::de::Error::custom)
    }
}

/* ---------------------------------- claims --------------------------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    UserAttested,
    ExternallySupported,
    Disputed,
    Inferred,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    CandidateDocument,
    CandidateAttestation,
    ExternalConfirmation,
    Inference,
}

/// A profile claim tied to evidence. Mirrors schemas/claim.schema.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub profile_revision: u64,
    pub text: String,
    pub status: ClaimStatus,
    pub evidence_ids: Vec<EvidenceId>,
    pub source_kind: SourceKind,
    pub requires_review: bool,
    pub supersedes: Option<ClaimId>,
}

impl Claim {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.profile_revision < 1 {
            return Err(DomainError::InvalidState(
                "profile_revision must be >= 1".to_string(),
            ));
        }
        if self.text.trim().is_empty() {
            return Err(DomainError::EmptyId("claim.text"));
        }
        // An externally supported claim must cite external-confirmation evidence.
        if self.status == ClaimStatus::ExternallySupported
            && (self.source_kind != SourceKind::ExternalConfirmation
                || self.evidence_ids.is_empty())
        {
            return Err(DomainError::UnsupportedClaimWithoutSource);
        }
        Ok(())
    }
}

/* ------------------------------- money amounts ------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmountBasis {
    Gross,
    Net,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayPeriod {
    Hourly,
    Monthly,
    Annual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationCategory {
    Base,
    Variable,
    Equity,
}

/// A money amount. Never compared across currency/period without explicit
/// normalization (MASTER_PLAN §5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Amount {
    /// ISO-4217 code, e.g. "CHF", "USD".
    pub currency: String,
    pub basis: AmountBasis,
    pub period: PayPeriod,
    pub category: CompensationCategory,
    pub min_minor_units: Option<i64>,
    pub max_minor_units: Option<i64>,
    /// "employer_published" | "estimated" | "user_supplied"
    pub source: String,
}

impl Amount {
    pub fn validate(&self) -> Result<(), DomainError> {
        let cur = self.currency.trim();
        if cur.len() != 3 || !cur.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(DomainError::InvalidCurrencyBasis);
        }
        if self.source.trim().is_empty() {
            return Err(DomainError::InvalidCurrencyBasis);
        }
        Ok(())
    }
}

/* ---------------------------- application states ---------------------------- */

/// Legible application transaction states (MASTER_PLAN §9 D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationState {
    Discovered,
    Shortlisted,
    Prepared,
    Approved,
    Submitting,
    Confirmed,
    Uncertain,
    ExternalAttested,
    Rejected,
    Interview,
    Offer,
}

impl ApplicationState {
    /// Parse strictly: the point of this type is that unknown states are rejected.
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| DomainError::InvalidState(s.to_string()))
    }

    /// Whether transitioning from `self` to `next` is on the allowed core path.
    pub fn may_transition_to(self, next: ApplicationState) -> bool {
        use ApplicationState as S;
        matches!(
            (self, next),
            (S::Discovered, S::Shortlisted)
                | (S::Shortlisted, S::Prepared)
                | (S::Prepared, S::Approved)
                | (S::Approved, S::Prepared) // expired/cancelled approval returns to Prepared
                | (S::Approved, S::Submitting)
                | (S::Submitting, S::Confirmed)
                | (S::Submitting, S::Uncertain)
                | (S::Uncertain, S::Confirmed) // reconciled via evidence
                | (S::Uncertain, S::Approved) // deliberate, recorded retry after review
                | (S::Discovered, S::ExternalAttested)
                | (S::Shortlisted, S::ExternalAttested)
                | (S::Confirmed, S::Rejected)
                | (S::Confirmed, S::Interview)
                | (S::ExternalAttested, S::Rejected)
                | (S::ExternalAttested, S::Interview)
                | (S::Interview, S::Offer)
                | (S::Interview, S::Rejected)
                | (S::Offer, S::Rejected)
        )
    }
}

/* ------------------------------ approval grants ----------------------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantOperation {
    Prefill,
    Submit,
    SendEmail,
    CreateEvent,
}

/// One-use permission bound to exact payload, packet, form schema and origin.
pub struct ApprovalGrant {
    pub id: GrantId,
    pub intent_id: IntentId,
    pub bound_payload_sha256: Sha256Digest,
    pub profile_revision: u64,
    pub packet_sha256: Sha256Digest,
    pub form_schema_sha256: Sha256Digest,
    pub destination_origin: String,
    pub operation: GrantOperation,
    pub nonce: String,
    /// True only when the grant came from an explicit user gesture.
    pub explicit_user_gesture: bool,
    pub consumed: bool,
    pub expired: bool,
}

impl ApprovalGrant {
    /// Enforcement: a grant may authorize exactly one write, once, only while
    /// valid, and only if the artifacts it binds still match.
    pub fn authorize(
        &mut self,
        intent_id: &IntentId,
        payload: &Sha256Digest,
        packet: &Sha256Digest,
        form_schema: &Sha256Digest,
        origin: &str,
        now_expired: bool,
    ) -> Result<(), DomainError> {
        if self.expired || now_expired {
            return Err(DomainError::UnboundApproval);
        }
        if self.consumed {
            return Err(DomainError::UnboundApproval);
        }
        if !self.explicit_user_gesture {
            return Err(DomainError::UnboundApproval);
        }
        if &self.intent_id != intent_id {
            return Err(DomainError::ApprovalNotBoundToPayload);
        }
        if &self.bound_payload_sha256 != payload
            || &self.packet_sha256 != packet
            || &self.form_schema_sha256 != form_schema
            || self.destination_origin != origin
        {
            return Err(DomainError::ApprovalNotBoundToPayload);
        }
        self.consumed = true;
        Ok(())
    }
}

/* ------------------------------ job observation ----------------------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Open,
    Closed,
    Stale,
    Unknown,
}

/// A point-in-time observation of a posting (HTTP 200 is not proof of Open).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobObservation {
    pub job_identity: JobIdentityId,
    pub source_url: String,
    pub observed_at: String, // RFC 3339
    pub content_hash: Sha256Digest,
    pub status: ObservationStatus,
    /// "http_get", "ats_api", "user_reported", ...
    pub method: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(c: char) -> Sha256Digest {
        Sha256Digest::new(c.to_string().repeat(64)).unwrap()
    }

    fn good_claim() -> Claim {
        Claim {
            id: ClaimId::new("c1").unwrap(),
            profile_revision: 1,
            text: "Led a team".into(),
            status: ClaimStatus::UserAttested,
            evidence_ids: vec![],
            source_kind: SourceKind::CandidateAttestation,
            requires_review: false,
            supersedes: None,
        }
    }

    #[test]
    fn rejects_unknown_application_state() {
        assert!(matches!(
            ApplicationState::parse("maybe_sent"),
            Err(DomainError::InvalidState(_))
        ));
        assert!(ApplicationState::parse("confirmed").is_ok());
    }

    #[test]
    fn rejects_externally_supported_claim_without_external_source() {
        let mut c = good_claim();
        c.status = ClaimStatus::ExternallySupported; // no evidence, wrong kind
        assert_eq!(
            c.validate(),
            Err(DomainError::UnsupportedClaimWithoutSource)
        );
        c.source_kind = SourceKind::ExternalConfirmation;
        c.evidence_ids = vec![EvidenceId::new("e1").unwrap()];
        assert!(c.validate().is_ok());
    }

    #[test]
    fn rejects_invalid_currency_basis() {
        let mut a = Amount {
            currency: "CHF".into(),
            basis: AmountBasis::Gross,
            period: PayPeriod::Annual,
            category: CompensationCategory::Base,
            min_minor_units: Some(9_000_000),
            max_minor_units: None,
            source: "employer_published".into(),
        };
        assert!(a.validate().is_ok());
        a.currency = "chf".into(); // not uppercase ISO
        assert_eq!(a.validate(), Err(DomainError::InvalidCurrencyBasis));
        a.currency = "CHF".into();
        a.source = " ".into();
        assert_eq!(a.validate(), Err(DomainError::InvalidCurrencyBasis));
    }

    #[test]
    fn rejects_bad_sha256_formats() {
        assert!(Sha256Digest::new("abc").is_err());
        assert!(Sha256Digest::new("A".repeat(64)).is_err()); // uppercase
        assert!(Sha256Digest::new("g".repeat(64)).is_err());
        assert!(Sha256Digest::new("a".repeat(64)).is_ok());
    }

    #[test]
    fn approval_is_single_use_hash_bound_and_gesture_required() {
        let payload = digest('a');
        let packet = digest('b');
        let form = digest('c');
        let intent = IntentId::new("i1").unwrap();

        let mut grant = ApprovalGrant {
            id: GrantId::new("g1").unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: payload.clone(),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: form.clone(),
            destination_origin: "https://jobs.example.com".into(),
            operation: GrantOperation::Submit,
            nonce: "n".repeat(32),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };

        // Unbound: wrong intent
        assert!(grant
            .authorize(
                &IntentId::new("i2").unwrap(),
                &payload,
                &packet,
                &form,
                "https://jobs.example.com",
                false
            )
            .is_err());

        // First use succeeds
        assert!(grant
            .authorize(
                &intent,
                &payload,
                &packet,
                &form,
                "https://jobs.example.com",
                false
            )
            .is_ok());

        // Second use is rejected (one-use)
        assert!(grant
            .authorize(
                &intent,
                &payload,
                &packet,
                &form,
                "https://jobs.example.com",
                false
            )
            .is_err());

        // A grant without an explicit user gesture can never authorize.
        let mut forged = ApprovalGrant {
            id: GrantId::new("g2").unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: payload.clone(),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: form.clone(),
            destination_origin: "https://jobs.example.com".into(),
            operation: GrantOperation::Submit,
            nonce: "n".repeat(32),
            explicit_user_gesture: false,
            consumed: false,
            expired: false,
        };
        assert!(forged
            .authorize(
                &intent,
                &payload,
                &packet,
                &form,
                "https://jobs.example.com",
                false
            )
            .is_err());
    }

    #[test]
    fn state_machine_forbids_illegal_jumps() {
        use ApplicationState as S;
        assert!(S::Prepared.may_transition_to(S::Approved));
        assert!(S::Submitting.may_transition_to(S::Uncertain));
        // no rollback of an external write:
        assert!(!S::Submitting.may_transition_to(S::Prepared));
        // a generated document is never "confirmed":
        assert!(!S::Prepared.may_transition_to(S::Confirmed));
    }
}
