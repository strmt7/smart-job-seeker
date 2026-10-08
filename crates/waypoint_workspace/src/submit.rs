//! The submission stage: the only place an approval can be spent.
//!
//! Order of operations is the safety property, and it is enforced here:
//!   1. the live form is inspected (never trusted from memory);
//!   2. preflight re-checks grant binding, freshness, origin, form schema and
//!      required fields — a mismatch VOIDS the approval and reopens the job;
//!   3. only then is the grant consumed, durably, and the write intent journalled;
//!   4. fields are filled with typed commands only (no script execution);
//!   5. the write happens, and the outcome is either a *qualified* receipt
//!      (Confirmed) or Uncertain — never a hopeful "submitted".
//!
//! Two rules the code makes structural rather than aspirational:
//! - Nothing is ever filled on the candidate's behalf for a sensitive or
//!   consent field, and nothing is invented: a required field the packet
//!   cannot genuinely answer blocks the submission and names itself.
//! - After an ambiguous write there is no automatic retry. Leaving Uncertain
//!   requires a deliberate, journalled user action.

use waypoint_applications::form_mapping::{
    validate_mapping, BrowserCommand, FormField, FormFieldType, FormSchema, PacketValue,
    ProposedMapping,
};
use waypoint_applications::journal::JournalEntry;
use waypoint_applications::preflight::{preflight_check, PreflightInput};
use waypoint_applications::receipt::{verify_receipt, ReceiptVerdict};
use waypoint_applications::recovery::{reconcile_uncertain, AmbiguityInputs, RecoveryAction};
use waypoint_domain::{
    ApplicationState, ApprovalGrant, GrantId, GrantOperation, IntentId, JobIdentityId, Sha256Digest,
};
use waypoint_search::{freshness, Freshness};
use waypoint_store::work::StoredGrant;

use crate::{Workspace, WorkspaceError, STALE_AFTER_SECONDS};

/// The browser contract now lives in `waypoint_applications::driver`, beside
/// the form model it acts on, so a concrete browser crate can implement it
/// without depending on this pipeline.
pub use waypoint_applications::driver::{BrowserDriver, DriverError, LiveForm, SiteAck};

/// The result of attempting a submission.
#[derive(Debug, Clone, PartialEq)]
pub enum SubmitOutcome {
    /// Nothing was sent. Reasons are user-facing and specific.
    Refused {
        issues: Vec<String>,
        /// True when the refusal voided the approval (so the UI can say so).
        approval_voided: bool,
    },
    Confirmed {
        application_ref: String,
    },
    /// The write may or may not have landed. Never auto-retried.
    Uncertain {
        why: String,
    },
}

impl SubmitOutcome {
    pub fn is_sent(&self) -> bool {
        matches!(self, SubmitOutcome::Confirmed { .. })
    }
}

/// The digest a grant must carry to be spendable on this exact live form.
pub fn live_form_schema_digest(live: &LiveForm) -> Sha256Digest {
    Sha256Digest::of(&live.schema.schema_hash_input())
}

impl Workspace {
    /// Attempt to submit the approved packet for `job_id` through `driver`.
    pub fn submit_packet(
        &mut self,
        job_id: &JobIdentityId,
        driver: &mut dyn BrowserDriver,
        now_unix: i64,
    ) -> Result<SubmitOutcome, WorkspaceError> {
        let job = self
            .store
            .get_job(job_id)?
            .ok_or_else(|| WorkspaceError::NotFound(job_id.as_str().to_string()))?;
        if job.state != ApplicationState::Approved {
            return Err(WorkspaceError::IllegalTransition {
                from: job.state,
                to: ApplicationState::Submitting,
            });
        }
        let packet = self
            .store
            .packet_for_job(job_id)?
            .ok_or_else(|| WorkspaceError::PacketRequired(job_id.as_str().to_string()))?;
        let stored = self
            .store
            .grant_for_job(job_id)?
            .ok_or_else(|| WorkspaceError::Approval("no approval exists for this job".into()))?;
        if stored.expired || stored.consumed {
            return Ok(SubmitOutcome::Refused {
                issues: vec![if stored.consumed {
                    "this approval has already been used; approve again".into()
                } else {
                    "this approval was voided because something it relied on changed".into()
                }],
                approval_voided: stored.expired,
            });
        }
        let url = job
            .canonical_url
            .clone()
            .ok_or_else(|| WorkspaceError::Approval("the job has no destination URL".into()))?;

        // 1. Inspect the live form. Nothing is trusted from memory.
        let live = driver
            .open(&url)
            .map_err(|e| WorkspaceError::Approval(format!("could not open the page: {e}")))?;
        if !live.walls.is_empty() {
            return Ok(SubmitOutcome::Refused {
                issues: live
                    .walls
                    .iter()
                    .map(|w| format!("cannot proceed: {w} (this is left to you, by design)"))
                    .collect(),
                approval_voided: false,
            });
        }

        // 2. Plan the fill. Only what the packet genuinely answers; nothing is
        //    invented, and sensitive/consent fields are never auto-filled.
        let plan = plan_fill(&live.schema, &packet.body);

        // 3. Preflight at the boundary.
        let live_schema = live_form_schema_digest(&live);
        let packet_digest = Sha256Digest::new(stored.packet_sha256.clone())?;
        let intent_id = IntentId::new(stored.intent_id.clone())?;
        let mut grant = domain_grant(&stored)?;
        let observed_at = job
            .last_seen
            .as_deref()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        let job_fresh = matches!(
            freshness(observed_at, now_unix, STALE_AFTER_SECONDS),
            Freshness::Fresh { .. }
        );
        let required: Vec<(&str, bool)> = live
            .schema
            .fields
            .iter()
            .map(|f| (f.selector.as_str(), plan.satisfied(f)))
            .collect();
        // Field answers are deliberately NOT part of preflight. Preflight owns
        // the grant's binding (identity, freshness, schema, origin, hashes); a
        // missing answer is the candidate's to supply and must not burn their
        // approval. `plan_fill` + `validate_mapping` below is the authoritative
        // field check, and it also runs before anything is consumed.
        let _ = required;
        let input = PreflightInput {
            intent_id: &intent_id,
            job_identity: job_id.as_str(),
            job_fresh,
            expected_form_schema: &Sha256Digest::new(stored.form_schema_sha256.clone())?,
            found_form_schema: &live_schema,
            expected_packet: &packet_digest,
            found_packet: &packet_digest,
            expected_origin: &stored.destination_origin,
            found_origin: &live.origin,
            required_fields_present: &[],
        };
        if let Err(issues) = preflight_check(&mut grant, &input) {
            // A failed preflight voids the approval: persist that, reopen the
            // job to Prepared, and let the candidate decide what to do next.
            self.store
                .expire_grants_for_packets(std::slice::from_ref(&stored.packet_sha256))?;
            self.set_state(job_id, ApplicationState::Prepared)?;
            return Ok(SubmitOutcome::Refused {
                issues: issues.iter().map(describe_issue).collect(),
                approval_voided: true,
            });
        }
        if !plan.issues.is_empty() {
            // The form is unchanged and the approval still stands, but the
            // packet cannot answer it. Nothing has been sent.
            return Ok(SubmitOutcome::Refused {
                issues: plan.issues.clone(),
                approval_voided: false,
            });
        }

        // 4. Spend the approval (durably) BEFORE any fill or write, then record
        //    the intent. From here on, cancellation is not rollback.
        self.consume_grant(job_id, &packet_digest, &live_schema, &live.origin)
            .map_err(|e| WorkspaceError::Approval(e.to_string()))?;
        self.journal(job_id, ApplicationState::Approved, now_unix, "preflight_ok")?;
        self.journal(
            job_id,
            ApplicationState::Submitting,
            now_unix,
            "write_started",
        )?;
        // State now reflects that a write is in flight.
        self.set_state(job_id, ApplicationState::Submitting)?;

        // 5. Fill with typed commands only.
        for command in plan.commands() {
            if let Err(e) = driver.execute(&command) {
                // Fill failed before the write: nothing was sent. The approval
                // is spent, so a fresh user gesture is required — failing closed.
                self.journal(
                    job_id,
                    ApplicationState::Uncertain,
                    now_unix,
                    "fill_failed_before_write",
                )?;
                self.set_state(job_id, ApplicationState::Uncertain)?;
                return Ok(SubmitOutcome::Uncertain {
                    why: format!(
                        "the page could not be filled ({e}); nothing was sent, and a new approval is required"
                    ),
                });
            }
        }

        // 6. The write.
        match driver.commit() {
            Ok(ack) => match verify_receipt(ack.receipt_kind, &ack.payload) {
                ReceiptVerdict::Confirmed { application_ref } => {
                    self.journal(
                        job_id,
                        ApplicationState::Confirmed,
                        now_unix,
                        "receipt_confirmed",
                    )?;
                    self.set_state(job_id, ApplicationState::Confirmed)?;
                    Ok(SubmitOutcome::Confirmed { application_ref })
                }
                ReceiptVerdict::NotSufficient { why } => {
                    self.journal(
                        job_id,
                        ApplicationState::Uncertain,
                        now_unix,
                        "receipt_not_qualifying",
                    )?;
                    self.set_state(job_id, ApplicationState::Uncertain)?;
                    Ok(SubmitOutcome::Uncertain {
                        why: format!("the site did not give a qualifying confirmation: {why}"),
                    })
                }
            },
            Err(e) => {
                // Ambiguous: the write may or may not have landed.
                let action = reconcile_uncertain(&AmbiguityInputs {
                    write_started: true,
                    receipt_kind: None,
                    receipt_payload: None,
                });
                let why = match action {
                    RecoveryAction::StayUncertain { why } => why,
                    RecoveryAction::ReturnToPrepared { why } => why,
                    other => format!("{other:?}"),
                };
                self.journal(
                    job_id,
                    ApplicationState::Uncertain,
                    now_unix,
                    "timeout_after_write",
                )?;
                self.set_state(job_id, ApplicationState::Uncertain)?;
                Ok(SubmitOutcome::Uncertain {
                    why: format!("{} — never retried automatically ({e})", why),
                })
            }
        }
    }

    /// The candidate checked and the application did land (they saw the
    /// employer's acknowledgement). This is a deliberate user decision, so it
    /// is journalled as one — never inferred by the app.
    pub fn confirm_after_uncertain(
        &mut self,
        job_id: &JobIdentityId,
        now_unix: i64,
    ) -> Result<(), WorkspaceError> {
        let job = self
            .store
            .get_job(job_id)?
            .ok_or_else(|| WorkspaceError::NotFound(job_id.as_str().to_string()))?;
        if job.state != ApplicationState::Uncertain {
            return Err(WorkspaceError::IllegalTransition {
                from: job.state,
                to: ApplicationState::Confirmed,
            });
        }
        self.journal(
            job_id,
            ApplicationState::Confirmed,
            now_unix,
            "user_confirmed_landed_after_uncertain",
        )?;
        self.set_state(job_id, ApplicationState::Confirmed)?;
        Ok(())
    }

    /// The deliberate, recorded retry after an Uncertain outcome. Only the UI
    /// may call this, from a control that shows the duplicate-application risk.
    pub fn retry_after_uncertain(
        &mut self,
        job_id: &JobIdentityId,
        now_unix: i64,
    ) -> Result<(), WorkspaceError> {
        let job = self
            .store
            .get_job(job_id)?
            .ok_or_else(|| WorkspaceError::NotFound(job_id.as_str().to_string()))?;
        if job.state != ApplicationState::Uncertain {
            return Err(WorkspaceError::IllegalTransition {
                from: job.state,
                to: ApplicationState::Approved,
            });
        }
        // The reason prefix is what the journal's own invariant requires for a
        // post-write-boundary return to Approved.
        self.journal(
            job_id,
            ApplicationState::Approved,
            now_unix,
            "user_deliberate_retry_after_uncertain",
        )?;
        self.set_state(job_id, ApplicationState::Approved)?;
        Ok(())
    }

    /// The full submission history for one job, oldest first.
    pub fn submission_history(
        &self,
        job_id: &JobIdentityId,
    ) -> Result<Vec<waypoint_store::JournalRow>, WorkspaceError> {
        Ok(self.store.journal_entries(job_id.as_str())?)
    }

    pub(crate) fn journal(
        &mut self,
        job_id: &JobIdentityId,
        state: ApplicationState,
        now_unix: i64,
        reason: &str,
    ) -> Result<(), WorkspaceError> {
        // The in-memory journal enforces the transition and no-auto-retry
        // invariants; the database stores it append-only.
        let mut journal = waypoint_applications::journal::SubmissionJournal::new();
        for existing in self.store.journal_entries(job_id.as_str())? {
            journal
                .append(JournalEntry {
                    application_id: existing.application_id.clone(),
                    state: existing.state,
                    timestamp: existing.timestamp.clone(),
                    reason: existing.reason.clone(),
                })
                .map_err(|e| WorkspaceError::Journal(format!("{e:?}")))?;
        }
        journal
            .append(JournalEntry {
                application_id: job_id.as_str().to_string(),
                state,
                timestamp: now_unix.to_string(),
                reason: reason.to_string(),
            })
            .map_err(|e| WorkspaceError::Journal(format!("{e:?}")))?;
        self.store
            .append_journal(job_id.as_str(), state, &now_unix.to_string(), reason)?;
        Ok(())
    }
}

fn domain_grant(stored: &StoredGrant) -> Result<ApprovalGrant, WorkspaceError> {
    Ok(ApprovalGrant {
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
    })
}

fn describe_issue(issue: &waypoint_applications::preflight::PreflightIssue) -> String {
    use waypoint_applications::preflight::PreflightIssue as I;
    match issue {
        I::JobStale { identity } => format!(
            "the posting ({identity}) is stale; re-check it before applying"
        ),
        I::FormSchemaChanged { expected, found } => format!(
            "the application form changed since you approved it ({} -> {}); approve the current form",
            &expected[..12.min(expected.len())],
            &found[..12.min(found.len())]
        ),
        I::DestinationChanged { expected, found } => {
            format!("the destination changed ({expected} -> {found})")
        }
        I::PacketChanged { .. } => {
            "the packet changed since you approved it; approve it again".into()
        }
        I::RequiredFieldMissing { field } => {
            format!("the form requires \"{field}\" and the packet cannot answer it")
        }
        I::GrantExpired => "the approval expired".into(),
        I::GrantConsumed => "the approval was already used".into(),
        I::GrantNotUserAuthorized => "the approval was not authorized by you".into(),
        I::GrantMismatch => "the approval does not match this submission".into(),
    }
}

/// The fill plan: which fields the packet can honestly answer.
#[derive(Debug, Clone, Default)]
pub struct FillPlan {
    pub proposals: Vec<ProposedMapping>,
    /// Fields the candidate must answer themselves.
    pub needs_user: Vec<String>,
    pub issues: Vec<String>,
}

impl FillPlan {
    pub fn satisfied(&self, field: &FormField) -> bool {
        if field.sensitive {
            // Never auto-filled: a sensitive field is satisfied only by the
            // candidate's own answer, which this stage does not have.
            return false;
        }
        self.proposals
            .iter()
            .any(|p| p.field_selector == field.selector && !matches!(p.value, PacketValue::None))
    }

    pub fn commands(&self) -> Vec<BrowserCommand> {
        self.proposals
            .iter()
            .filter(|p| !matches!(p.value, PacketValue::None))
            .map(|p| match &p.value {
                PacketValue::FileRef { name } => BrowserCommand::UploadFile {
                    selector: p.field_selector.clone(),
                    file_name: name.clone(),
                },
                other => BrowserCommand::Fill {
                    selector: p.field_selector.clone(),
                    value: other.clone(),
                },
            })
            .collect()
    }
}

/// Decide what to fill. Conservative by construction: the only things the app
/// supplies are the packet's own text (for cover-letter style prompts) and the
/// packet file (for résumé uploads). Names, consent, salary expectations and
/// anything sensitive belong to the candidate.
pub fn plan_fill(schema: &FormSchema, packet_body: &str) -> FillPlan {
    let mut plan = FillPlan::default();
    let body_for_form: String = packet_body.chars().take(4000).collect();

    for field in &schema.fields {
        if field.sensitive {
            plan.needs_user.push(field.selector.clone());
            continue;
        }
        let label = field.label.to_lowercase();
        let proposal = match &field.field_type {
            FormFieldType::File if label.contains("resume") || label.contains("cv") => {
                Some(PacketValue::FileRef {
                    name: "waypoint-packet.docx".into(),
                })
            }
            FormFieldType::TextArea
                if label.contains("cover")
                    || label.contains("letter")
                    || label.contains("message")
                    || label.contains("why")
                    || label.contains("motivation") =>
            {
                Some(PacketValue::Text(body_for_form.clone()))
            }
            _ => None,
        };
        if let Some(value) = proposal {
            plan.proposals.push(ProposedMapping {
                field_selector: field.selector.clone(),
                value,
            });
        }
    }

    // Validate against the real schema; report anything still unanswered.
    if let Err(errors) = validate_mapping(schema, &plan.proposals, &[]) {
        for e in errors {
            use waypoint_applications::form_mapping::MappingError as M;
            plan.issues.push(match e {
                M::SensitiveFieldNeedsUserAnswer { selector } => {
                    format!("you must answer \"{selector}\" yourself (it is never auto-filled)")
                }
                M::MissingRequired { selector } => {
                    format!("the form requires \"{selector}\", which the packet does not contain")
                }
                M::TypeMismatch { selector, why } => {
                    format!("field \"{selector}\" cannot accept what the packet provides: {why}")
                }
                M::UnknownField { selector } => {
                    format!("the plan referenced a field that is not on the page: \"{selector}\"")
                }
            });
        }
    }
    plan
}
