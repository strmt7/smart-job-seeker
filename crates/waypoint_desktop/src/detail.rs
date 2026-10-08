//! View types for the "act on a job" surface: facts, packet status and
//! approval status, plus pure mapping from the pipeline's plain data.
//!
//! Nothing here performs work or reaches the network/store; the controller
//! produces these values on a worker and the shell only renders them.

use waypoint_domain::ApplicationState;
use waypoint_store::work::{StoredClaim, StoredGrant, StoredPacket};
use waypoint_store::{JournalRow, StoredJob};

use crate::view_model::state_label;

/// One candidate fact, render-ready.
#[derive(Debug, Clone, PartialEq)]
pub struct FactRow {
    pub id: String,
    pub text: String,
    pub profile_revision: u64,
    /// True when this fact is backed only by model inference: it cannot
    /// support an approvable packet until the candidate confirms it.
    pub inferred: bool,
}

/// A prepared packet's identity and health.
#[derive(Debug, Clone, PartialEq)]
pub struct PacketView {
    /// First 12 hex chars: enough to compare by eye, short enough to read.
    pub hash_prefix: String,
    pub created_at: String,
    /// Set when a cited fact changed after this packet was prepared.
    pub invalidated_at: Option<String>,
    pub supported_spans: usize,
    /// Reasons approval would currently be refused.
    pub unsupported: Vec<String>,
}

impl PacketView {
    pub fn is_approvable(&self) -> bool {
        self.unsupported.is_empty() && self.supported_spans > 0 && self.invalidated_at.is_none()
    }
}

/// An approval's state, in words the candidate can act on.
#[derive(Debug, Clone, PartialEq)]
pub struct GrantView {
    pub id: String,
    pub destination_origin: String,
    pub consumed: bool,
    pub expired: bool,
}

impl GrantView {
    pub fn state_label(&self) -> &'static str {
        if self.expired {
            "voided — a fact it relied on changed"
        } else if self.consumed {
            "used (single use)"
        } else {
            "live — approved by you, not yet submitted"
        }
    }
}

/// Everything the detail panel shows for the selected job.
#[derive(Debug, Clone, PartialEq)]
pub struct JobDetail {
    pub identity: String,
    pub title: String,
    pub employer: String,
    pub state: ApplicationState,
    pub state_label: String,
    pub remote: bool,
    pub packet: Option<PacketView>,
    pub grant: Option<GrantView>,
    /// The append-only submission history, oldest first: what actually
    /// happened, with the machine-checkable reason for each step.
    pub history: Vec<HistoryRow>,
}

/// One journalled step, render-ready.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    pub at: String,
    pub state_label: String,
    pub reason: String,
}

impl JobDetail {
    /// Whether an actual submission may be attempted right now: the job is
    /// approved, the approval is live, and the packet is still valid.
    pub fn can_submit(&self) -> bool {
        self.state == ApplicationState::Approved
            && self
                .grant
                .as_ref()
                .is_some_and(|g| !g.expired && !g.consumed)
            && self
                .packet
                .as_ref()
                .is_some_and(|p| p.invalidated_at.is_none())
    }

    /// Whether the candidate must deliberately decide about an ambiguous write.
    pub fn needs_reconciliation(&self) -> bool {
        self.state == ApplicationState::Uncertain
    }

    /// The legal next steps for this job, as button labels. Mirrors the state
    /// machine rather than inventing its own rules.
    pub fn next_actions(&self) -> Vec<&'static str> {
        match self.state {
            ApplicationState::Discovered => vec!["Shortlist"],
            ApplicationState::Shortlisted => vec!["Prepare packet"],
            ApplicationState::Prepared => {
                if self.packet.as_ref().is_some_and(|p| p.is_approvable()) {
                    vec!["Approve", "Prepare packet"]
                } else {
                    vec!["Prepare packet"]
                }
            }
            ApplicationState::Approved => {
                if self
                    .grant
                    .as_ref()
                    .is_some_and(|g| !g.expired && !g.consumed)
                {
                    vec!["Submit (browser stage)", "Export DOCX"]
                } else {
                    vec!["Prepare packet"]
                }
            }
            _ => vec!["Export DOCX"],
        }
    }
}

pub fn facts_from(claims: &[StoredClaim]) -> Vec<FactRow> {
    claims
        .iter()
        .map(|c| FactRow {
            id: c.id.clone(),
            text: c.text.clone(),
            profile_revision: c.profile_revision,
            inferred: c.source_kind == waypoint_domain::SourceKind::Inference,
        })
        .collect()
}

pub fn detail_from(
    job: &StoredJob,
    packet: Option<&StoredPacket>,
    readiness: Option<(usize, Vec<String>)>,
    grant: Option<&StoredGrant>,
    history: &[JournalRow],
) -> JobDetail {
    JobDetail {
        identity: job.id.as_str().to_string(),
        title: job.title.clone(),
        employer: job.employer.clone(),
        state: job.state,
        state_label: state_label(job.state).to_string(),
        remote: job.remote,
        packet: packet.map(|p| {
            let (supported_spans, unsupported) = readiness.unwrap_or((0, vec![]));
            PacketView {
                hash_prefix: p.packet_sha256.chars().take(12).collect(),
                created_at: p.created_at.clone(),
                invalidated_at: p.invalidated_at.clone(),
                supported_spans,
                unsupported,
            }
        }),
        grant: grant.map(|g| GrantView {
            id: g.id.clone(),
            destination_origin: g.destination_origin.clone(),
            consumed: g.consumed,
            expired: g.expired,
        }),
        history: history
            .iter()
            .map(|row| HistoryRow {
                at: row.timestamp.clone(),
                state_label: state_label(row.state).to_string(),
                reason: row.reason.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::ClaimStatus;
    use waypoint_domain::SourceKind;

    fn job(state: ApplicationState) -> StoredJob {
        StoredJob {
            id: waypoint_domain::JobIdentityId::new("j1").unwrap(),
            title: "Backend Engineer".into(),
            employer: "acme".into(),
            location: "Remote".into(),
            state,
            canonical_url: None,
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: true,
            source: "greenhouse".into(),
            requisition_id: None,
            posted_at: None,
        }
    }

    fn packet(invalidated: bool) -> StoredPacket {
        StoredPacket {
            job_id: "j1".into(),
            packet_sha256: "abcdef1234567890".into(),
            doc_json: "{}".into(),
            body: "body".into(),
            cited_claim_ids: vec!["c1".into()],
            profile_revision: 1,
            created_at: "100".into(),
            invalidated_at: if invalidated {
                Some("200".into())
            } else {
                None
            },
        }
    }

    #[test]
    fn inferred_facts_are_marked_for_the_candidate() {
        let claims = vec![
            StoredClaim {
                id: "c1".into(),
                profile_revision: 1,
                text: "Nine years of Rust".into(),
                status: ClaimStatus::UserAttested,
                source_kind: SourceKind::CandidateAttestation,
                requires_review: false,
                supersedes: None,
            },
            StoredClaim {
                id: "c2".into(),
                profile_revision: 1,
                text: "Probably knows Kubernetes".into(),
                status: ClaimStatus::Inferred,
                source_kind: SourceKind::Inference,
                requires_review: true,
                supersedes: None,
            },
        ];
        let rows = facts_from(&claims);
        assert!(!rows[0].inferred);
        assert!(rows[1].inferred, "inference-backed facts must be labelled");
    }

    #[test]
    fn an_invalidated_or_unsupported_packet_is_not_approvable() {
        let clean = PacketView {
            hash_prefix: "abc".into(),
            created_at: "1".into(),
            invalidated_at: None,
            supported_spans: 1,
            unsupported: vec![],
        };
        assert!(clean.is_approvable());

        let stale = PacketView {
            invalidated_at: Some("2".into()),
            ..clean.clone()
        };
        assert!(
            !stale.is_approvable(),
            "a stale packet may never be approved"
        );

        let unsupported = PacketView {
            unsupported: vec!["cited claim c9 is Disputed: \"x\"".into()],
            ..clean.clone()
        };
        assert!(!unsupported.is_approvable());

        let empty = PacketView {
            supported_spans: 0,
            ..clean
        };
        assert!(!empty.is_approvable());
    }

    #[test]
    fn grant_labels_say_what_the_candidate_needs_to_know() {
        let live = GrantView {
            id: "g".into(),
            destination_origin: "https://x".into(),
            consumed: false,
            expired: false,
        };
        assert!(live.state_label().contains("not yet submitted"));
        assert!(GrantView {
            consumed: true,
            ..live.clone()
        }
        .state_label()
        .contains("single use"));
        assert!(GrantView {
            expired: true,
            ..live
        }
        .state_label()
        .contains("voided"));
    }

    #[test]
    fn next_actions_follow_the_state_machine() {
        let detail = |state, approvable: bool, grant_live: bool| {
            let (invalidated, unsupported, supported) = if approvable {
                (None, vec![], 1)
            } else {
                (Some("t".to_string()), vec![], 0)
            };
            JobDetail {
                identity: "j1".into(),
                title: "t".into(),
                employer: "e".into(),
                state,
                state_label: state_label(state).into(),
                remote: true,
                packet: Some(PacketView {
                    hash_prefix: "a".into(),
                    created_at: "1".into(),
                    invalidated_at: invalidated,
                    supported_spans: supported,
                    unsupported,
                }),
                grant: Some(GrantView {
                    id: "g".into(),
                    destination_origin: "https://x".into(),
                    consumed: !grant_live,
                    expired: !grant_live,
                }),
                history: vec![],
            }
        };

        assert_eq!(
            detail(ApplicationState::Discovered, false, false).next_actions(),
            vec!["Shortlist"]
        );
        assert_eq!(
            detail(ApplicationState::Shortlisted, false, false).next_actions(),
            vec!["Prepare packet"]
        );
        // Prepared with unsupported evidence: approval is not even offered.
        assert_eq!(
            detail(ApplicationState::Prepared, false, false).next_actions(),
            vec!["Prepare packet"]
        );
        assert_eq!(
            detail(ApplicationState::Prepared, true, false).next_actions(),
            vec!["Approve", "Prepare packet"]
        );
        assert!(detail(ApplicationState::Approved, true, true)
            .next_actions()
            .contains(&"Submit (browser stage)"));
        // A voided approval must not offer submission again.
        assert_eq!(
            detail(ApplicationState::Approved, true, false).next_actions(),
            vec!["Prepare packet"]
        );
    }

    #[test]
    fn detail_exposes_the_packet_hash_prefix_and_readiness() {
        let detail = detail_from(
            &job(ApplicationState::Prepared),
            Some(&packet(false)),
            Some((2, vec!["problem".into()])),
            None,
            &[],
        );
        let p = detail.packet.as_ref().unwrap();
        assert_eq!(p.hash_prefix, "abcdef123456", "12 chars for eyeballing");
        assert_eq!(p.supported_spans, 2);
        assert!(!p.is_approvable());
        assert!(detail.grant.is_none());
    }
}
