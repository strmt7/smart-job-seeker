//! The "act on a job" workflow: facts → prepared packet → approval → export,
//! with correction propagation (D1) and one-use grants (D2) wired through
//! persistence.
//!
//! The rules this module enforces, rather than merely describing:
//! - A packet may only be prepared for a job the candidate has shortlisted
//!   (the state machine is walked, never jumped).
//! - A packet is a hash of its *content*, so any change produces a new hash.
//! - Approval requires zero unsupported spans and at least one attested fact:
//!   you cannot approve an empty packet, and you cannot approve a claim that
//!   only the model inferred.
//! - A grant is bound to packet + form schema + destination origin, is
//!   explicit-user-gesture only, and is consumed exactly once — across
//!   restarts, because consumption is persisted.
//! - Correcting a fact invalidates exactly the packets that cited it, expires
//!   exactly the grants bound to those packets, reopens those jobs to
//!   `Prepared`, and never touches anything already submitted.

use std::path::Path;

use waypoint_domain::{
    sha256_hex, ApplicationState, Claim, ClaimId, ClaimStatus, GrantId, GrantOperation, IntentId,
    JobIdentityId, Sha256Digest, SourceKind,
};
use waypoint_materials::export::to_docx_bytes;
use waypoint_materials::tree::{DocNode, DocNodeKind, SemanticDocument};
use waypoint_materials::validation::{validate_claim_support, ValidationOutcome};
use waypoint_store::work::{StoredClaim, StoredGrant, StoredPacket};

use crate::{Workspace, WorkspaceError};

/// Where a fact came from. Only attested facts can support an approvable
/// packet; model-inferred facts must be confirmed by the candidate first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactOrigin {
    UserAttested,
    ModelInferred,
}

/// A packet as prepared, with its evidence report.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedPacket {
    pub job_id: String,
    pub packet_sha256: String,
    pub body: String,
    pub cited_claims: Vec<String>,
    pub supported_spans: usize,
    /// Human-readable reasons a span is not yet supported.
    pub unsupported: Vec<String>,
    pub profile_revision: u64,
    pub created_at: String,
}

impl PreparedPacket {
    pub fn is_approvable(&self) -> bool {
        self.unsupported.is_empty() && self.supported_spans > 0
    }
}

/// The evidence state of a stored packet, recomputed against current facts.
#[derive(Debug, Clone, PartialEq)]
pub struct PacketReadiness {
    pub supported_spans: usize,
    pub unsupported: Vec<String>,
    /// Set when a cited fact changed after the packet was prepared.
    pub invalidated_at: Option<String>,
}

impl PacketReadiness {
    pub fn is_approvable(&self) -> bool {
        self.unsupported.is_empty() && self.supported_spans > 0 && self.invalidated_at.is_none()
    }
}

/// What a fact correction did, in full, so the UI can report it honestly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CorrectionReport {
    pub claim_id: String,
    pub new_claim_id: String,
    pub new_profile_revision: u64,
    /// Packets that cited the corrected fact and were marked stale.
    pub invalidated_packets: Vec<String>,
    /// Approvals that were expired as a result.
    pub expired_grants: Vec<String>,
    /// Jobs reopened from `Approved` back to `Prepared`.
    pub reopened_to_prepared: Vec<String>,
    /// Already-submitted applications that mention the fact. Listed, never
    /// modified — the employer already has them.
    pub submitted_untouched: Vec<String>,
}

impl Workspace {
    /* --------------------------------- facts -------------------------------- */

    pub fn facts(&self) -> Result<Vec<StoredClaim>, WorkspaceError> {
        Ok(self.store.active_claims()?)
    }

    pub fn fact_history(&self) -> Result<Vec<StoredClaim>, WorkspaceError> {
        Ok(self.store.all_claims()?)
    }

    pub fn profile_revision(&self) -> Result<u64, WorkspaceError> {
        Ok(self.store.profile_revision()?)
    }

    /// Record a candidate fact. `ModelInferred` facts are persisted but cannot
    /// support an approvable packet until the candidate attests them.
    pub fn add_fact(&mut self, text: &str, origin: FactOrigin) -> Result<String, WorkspaceError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(WorkspaceError::EmptyFact);
        }
        let revision = self.store.profile_revision()?;
        let fingerprint = &sha256_hex(text.as_bytes())[..12];
        let id = format!("c{revision}-{fingerprint}");
        let (status, source_kind, requires_review) = match origin {
            FactOrigin::UserAttested => (
                ClaimStatus::UserAttested,
                SourceKind::CandidateAttestation,
                false,
            ),
            FactOrigin::ModelInferred => (ClaimStatus::Inferred, SourceKind::Inference, true),
        };
        self.store.insert_claim(&StoredClaim {
            id: id.clone(),
            profile_revision: revision,
            text: text.to_string(),
            status,
            source_kind,
            requires_review,
            supersedes: None,
        })?;
        Ok(id)
    }

    /// Correct a fact: supersede it, advance the profile revision, then
    /// propagate the change across everything that cited it (D1).
    pub fn correct_fact(
        &mut self,
        claim_id: &str,
        new_text: &str,
        now_unix: i64,
    ) -> Result<CorrectionReport, WorkspaceError> {
        let new_text = new_text.trim();
        if new_text.is_empty() {
            return Err(WorkspaceError::EmptyFact);
        }
        let old = self
            .store
            .claim(claim_id)?
            .ok_or_else(|| WorkspaceError::UnknownFact(claim_id.to_string()))?;

        let new_revision = self.store.bump_profile_revision()?;
        let new_id = format!("c{new_revision}-{}", &sha256_hex(new_text.as_bytes())[..12]);
        self.store.insert_claim(&StoredClaim {
            id: new_id.clone(),
            profile_revision: new_revision,
            text: new_text.to_string(),
            status: ClaimStatus::UserAttested,
            source_kind: SourceKind::CandidateAttestation,
            requires_review: false,
            supersedes: Some(old.id.clone()),
        })?;

        let mut report = CorrectionReport {
            claim_id: old.id.clone(),
            new_claim_id: new_id,
            new_profile_revision: new_revision,
            ..Default::default()
        };
        let now = now_unix.to_string();

        for packet in self.store.packets_citing_claim(&old.id)? {
            let job_id = JobIdentityId::new(packet.job_id.clone())?;
            let Some(job) = self.store.get_job(&job_id)? else {
                continue;
            };
            if is_submitted(job.state) {
                // The employer already has this. Recorded, never rewritten.
                report.submitted_untouched.push(packet.job_id.clone());
                continue;
            }
            if self.store.invalidate_packet(&packet.job_id, &now)? {
                report.invalidated_packets.push(packet.job_id.clone());
            }
            report.expired_grants.extend(
                self.store
                    .expire_grants_for_packets(std::slice::from_ref(&packet.packet_sha256))?,
            );
            if job.state == ApplicationState::Approved {
                // The approval is void, so the job returns to Prepared.
                if self.set_state(&job_id, ApplicationState::Prepared)? {
                    report.reopened_to_prepared.push(packet.job_id.clone());
                }
            }
        }
        Ok(report)
    }

    /* -------------------------------- packets ------------------------------- */

    pub fn packet(&self, job_id: &JobIdentityId) -> Result<Option<StoredPacket>, WorkspaceError> {
        Ok(self.store.packet_for_job(job_id)?)
    }

    pub fn grant(&self, job_id: &JobIdentityId) -> Result<Option<StoredGrant>, WorkspaceError> {
        Ok(self.store.grant_for_job(job_id)?)
    }

    /// Recompute a stored packet's evidence state against the facts in force.
    /// Used by both the UI (to explain itself) and `approve_packet` (to gate).
    pub fn packet_readiness(
        &self,
        job_id: &JobIdentityId,
    ) -> Result<Option<PacketReadiness>, WorkspaceError> {
        let Some(packet) = self.store.packet_for_job(job_id)? else {
            return Ok(None);
        };
        let document: SemanticDocument = serde_json::from_str(&packet.doc_json)
            .map_err(|e| WorkspaceError::Export(format!("stored packet unreadable: {e}")))?;
        let claims = self.store.active_claims()?;
        let outcomes = validate_claim_support(&document, &domain_claims(&claims));
        let unsupported = outcomes
            .iter()
            .filter_map(|o| match o {
                ValidationOutcome::Unsupported { text, reason, .. } => {
                    Some(format!("{reason}: \"{text}\""))
                }
                ValidationOutcome::LockedUnsupported { text, .. } => {
                    Some(format!("locked wording is unsupported: \"{text}\""))
                }
                ValidationOutcome::Supported { .. } => None,
            })
            .collect();
        let supported_spans = outcomes
            .iter()
            .filter(|o| matches!(o, ValidationOutcome::Supported { .. }))
            .count();
        Ok(Some(PacketReadiness {
            supported_spans,
            unsupported,
            invalidated_at: packet.invalidated_at,
        }))
    }

    /// Build (or rebuild) the packet for a job from the candidate's current
    /// facts. Requires the job to be shortlisted first.
    pub fn prepare_packet(
        &mut self,
        job_id: &JobIdentityId,
        now_unix: i64,
    ) -> Result<PreparedPacket, WorkspaceError> {
        let job = self
            .store
            .get_job(job_id)?
            .ok_or_else(|| WorkspaceError::NotFound(job_id.as_str().to_string()))?;
        match job.state {
            ApplicationState::Shortlisted | ApplicationState::Prepared => {}
            other => {
                return Err(WorkspaceError::IllegalTransition {
                    from: other,
                    to: ApplicationState::Prepared,
                })
            }
        }
        let claims = self.store.active_claims()?;
        let revision = self.store.profile_revision()?;
        let document = self.build_document(&job, &claims, revision);
        let outcomes = validate_claim_support(&document, &domain_claims(&claims));
        let unsupported: Vec<String> = outcomes
            .iter()
            .filter_map(|o| match o {
                ValidationOutcome::Unsupported { text, reason, .. } => {
                    Some(format!("{reason}: \"{text}\""))
                }
                ValidationOutcome::LockedUnsupported { text, .. } => {
                    Some(format!("locked wording is unsupported: \"{text}\""))
                }
                ValidationOutcome::Supported { .. } => None,
            })
            .collect();
        let supported_spans = outcomes
            .iter()
            .filter(|o| matches!(o, ValidationOutcome::Supported { .. }))
            .count();

        let body = document.to_plain_text();
        let doc_json =
            serde_json::to_string(&document).map_err(|e| WorkspaceError::Export(e.to_string()))?;
        let cited_claims: Vec<String> = claims
            .iter()
            .filter(|c| {
                // Stored ids are validated on insert; a malformed one simply
                // cannot be cited rather than panicking the pipeline.
                ClaimId::new(c.id.clone())
                    .map(|id| !document.spans_citing(&id).is_empty())
                    .unwrap_or(false)
            })
            .map(|c| c.id.clone())
            .collect();
        let packet_sha256 = packet_hash(&body, &cited_claims, document.profile_revision);
        let created_at = now_unix.to_string();

        let stored = StoredPacket {
            job_id: job_id.as_str().to_string(),
            packet_sha256: packet_sha256.clone(),
            doc_json,
            body: body.clone(),
            cited_claim_ids: cited_claims.clone(),
            profile_revision: document.profile_revision,
            created_at: created_at.clone(),
            invalidated_at: None,
        };
        self.store.upsert_packet(&stored)?;
        self.set_state(job_id, ApplicationState::Prepared)?;

        Ok(PreparedPacket {
            job_id: job_id.as_str().to_string(),
            packet_sha256,
            body,
            cited_claims,
            supported_spans,
            unsupported,
            profile_revision: document.profile_revision,
            created_at,
        })
    }

    /// Export the stored packet as a real DOCX (OOXML zip). Returns byte count.
    pub fn export_packet(
        &self,
        job_id: &JobIdentityId,
        out_path: &Path,
    ) -> Result<usize, WorkspaceError> {
        let packet = self
            .store
            .packet_for_job(job_id)?
            .ok_or_else(|| WorkspaceError::PacketRequired(job_id.as_str().to_string()))?;
        let document: SemanticDocument = serde_json::from_str(&packet.doc_json)
            .map_err(|e| WorkspaceError::Export(format!("stored packet unreadable: {e}")))?;
        let bytes = to_docx_bytes(&document);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out_path, &bytes)?;
        Ok(bytes.len())
    }

    /* ------------------------------- approvals ------------------------------ */

    /// Create a one-use approval for a prepared packet. This is only reachable
    /// from an explicit user gesture in the UI; there is no model-facing path.
    pub fn approve_packet(
        &mut self,
        job_id: &JobIdentityId,
        destination_origin: &str,
        form_schema: &Sha256Digest,
        now_unix: i64,
    ) -> Result<StoredGrant, WorkspaceError> {
        let job = self
            .store
            .get_job(job_id)?
            .ok_or_else(|| WorkspaceError::NotFound(job_id.as_str().to_string()))?;
        if job.state != ApplicationState::Prepared {
            return Err(WorkspaceError::IllegalTransition {
                from: job.state,
                to: ApplicationState::Approved,
            });
        }
        let packet = self
            .store
            .packet_for_job(job_id)?
            .ok_or_else(|| WorkspaceError::PacketRequired(job_id.as_str().to_string()))?;
        if packet.invalidated_at.is_some() {
            return Err(WorkspaceError::PacketInvalidated(packet.job_id.clone()));
        }

        // Re-validate against current facts: what was approvable when prepared
        // may not be approvable now.
        let readiness = self
            .packet_readiness(job_id)?
            .ok_or_else(|| WorkspaceError::PacketRequired(job_id.as_str().to_string()))?;
        if !readiness.is_approvable() {
            return Err(WorkspaceError::UnsupportedClaims(
                if readiness.unsupported.is_empty() {
                    vec!["packet cites no attested facts".into()]
                } else {
                    readiness.unsupported
                },
            ));
        }

        let seq = self.store.grant_count_for_job(job_id)? + 1;
        let short = &packet.packet_sha256[..12];
        let grant = StoredGrant {
            id: format!("g-{short}-{seq}"),
            job_id: job_id.as_str().to_string(),
            intent_id: format!("intent-{short}"),
            // The payload and the packet are the same artifact at this stage;
            // a later stage that fills a form binds the rendered payload.
            bound_payload_sha256: packet.packet_sha256.clone(),
            packet_sha256: packet.packet_sha256.clone(),
            form_schema_sha256: form_schema.as_str().to_string(),
            destination_origin: destination_origin.to_string(),
            profile_revision: packet.profile_revision,
            nonce: format!("{now_unix}-{short}"),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        self.store.insert_grant(&grant)?;
        self.set_state(job_id, ApplicationState::Approved)?;
        Ok(grant)
    }

    /// Spend an approval. Refuses if the grant is expired, already consumed, or
    /// not bound to exactly this payload, form schema and origin — and records
    /// the consumption so a restart cannot resurrect it.
    pub fn consume_grant(
        &mut self,
        job_id: &JobIdentityId,
        payload: &Sha256Digest,
        form_schema: &Sha256Digest,
        destination_origin: &str,
    ) -> Result<(), WorkspaceError> {
        let stored = self
            .store
            .grant_for_job(job_id)?
            .ok_or_else(|| WorkspaceError::Approval("no approval exists for this job".into()))?;
        let mut grant = waypoint_domain::ApprovalGrant {
            id: GrantId::new(stored.id.clone())?,
            intent_id: IntentId::new(stored.intent_id.clone())?,
            bound_payload_sha256: Sha256Digest::new(stored.bound_payload_sha256.clone())?,
            profile_revision: stored.profile_revision,
            packet_sha256: Sha256Digest::new(stored.packet_sha256.clone())?,
            form_schema_sha256: Sha256Digest::new(stored.form_schema_sha256.clone())?,
            destination_origin: stored.destination_origin.clone(),
            operation: GrantOperation::Submit,
            nonce: stored.nonce.clone(),
            explicit_user_gesture: stored.explicit_user_gesture,
            consumed: stored.consumed,
            expired: stored.expired,
        };
        grant
            .authorize(
                &IntentId::new(stored.intent_id.clone())?,
                payload,
                &Sha256Digest::new(stored.packet_sha256.clone())?,
                form_schema,
                destination_origin,
                false,
            )
            .map_err(|e| WorkspaceError::Approval(e.to_string()))?;
        // Persist the spend. If this write fails, the approval stays live and
        // the caller must not proceed — failing closed.
        self.store.mark_grant_consumed(&grant.id)?;
        Ok(())
    }

    /* -------------------------------- helpers ------------------------------- */

    fn build_document(
        &self,
        job: &waypoint_store::StoredJob,
        claims: &[StoredClaim],
        profile_revision: u64,
    ) -> SemanticDocument {
        let mut root = DocNode {
            id: "root".into(),
            kind: DocNodeKind::Document,
            text: String::new(),
            claim_ids: vec![],
            locked: false,
            children: vec![],
        };
        root.children.push(DocNode::heading("Professional summary"));
        for c in claims {
            let claim_id = ClaimId::new(c.id.clone()).expect("stored claim ids are valid");
            root.children
                .push(DocNode::text_span(c.text.clone(), Some(claim_id)));
        }
        root.children.push(DocNode::heading("Target"));
        root.children.push(DocNode::text_span(
            format!("Applied for {} at {}.", job.title, job.employer),
            None,
        ));
        SemanticDocument {
            id: format!("packet-{}", job.id.as_str()),
            kind: "cover_letter".into(),
            root,
            profile_revision,
        }
    }
}

fn is_submitted(state: ApplicationState) -> bool {
    matches!(
        state,
        ApplicationState::Submitting
            | ApplicationState::Confirmed
            | ApplicationState::Uncertain
            | ApplicationState::ExternalAttested
            | ApplicationState::Rejected
            | ApplicationState::Interview
            | ApplicationState::Offer
    )
}

/// Content-addressed packet identity: changes whenever the facts, the cited
/// set or the profile revision change.
fn packet_hash(body: &str, cited: &[String], profile_revision: u64) -> String {
    let mut material = format!("rev:{profile_revision}\n");
    let mut sorted = cited.to_vec();
    sorted.sort();
    material.push_str(&sorted.join(","));
    material.push('\n');
    material.push_str(body);
    sha256_hex(material.as_bytes())
}

/// Bridge persisted claims into the domain type used by validation.
fn domain_claims(claims: &[StoredClaim]) -> Vec<Claim> {
    claims
        .iter()
        .filter_map(|c| {
            Some(Claim {
                id: ClaimId::new(c.id.clone()).ok()?,
                profile_revision: c.profile_revision,
                text: c.text.clone(),
                status: c.status,
                evidence_ids: vec![],
                source_kind: c.source_kind,
                requires_review: c.requires_review,
                supersedes: c.supersedes.clone().and_then(|s| ClaimId::new(s).ok()),
            })
        })
        .collect()
}
