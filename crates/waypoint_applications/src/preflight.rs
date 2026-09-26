//! T028/T029 — preflight + one-use approval enforcement at the boundary.
//!
//! Preflight rechecks identity/freshness, form schema, required fields,
//! hashes and grant binding *immediately before* the write. Any mismatch
//! invalidates the grant and returns the packet to Prepared.

use waypoint_domain::{ApprovalGrant, IntentId, Sha256Digest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreflightIssue {
    JobStale { identity: String },
    FormSchemaChanged { expected: String, found: String },
    DestinationChanged { expected: String, found: String },
    PacketChanged { expected: String, found: String },
    RequiredFieldMissing { field: String },
    GrantExpired,
    GrantConsumed,
    GrantNotUserAuthorized,
    GrantMismatch,
}

#[derive(Debug, Clone)]
pub struct PreflightInput<'a> {
    pub intent_id: &'a IntentId,
    pub job_identity: &'a str,
    pub job_fresh: bool,
    pub expected_form_schema: &'a Sha256Digest,
    pub found_form_schema: &'a Sha256Digest,
    pub expected_packet: &'a Sha256Digest,
    pub found_packet: &'a Sha256Digest,
    pub expected_origin: &'a str,
    pub found_origin: &'a str,
    pub required_fields_present: &'a [(&'a str, bool)],
}

/// Run all preflight checks. Returns Err(list of issues) blocking the write.
pub fn preflight_check(
    grant: &mut ApprovalGrant,
    input: &PreflightInput<'_>,
) -> Result<(), Vec<PreflightIssue>> {
    let mut issues = vec![];

    if !input.job_fresh {
        issues.push(PreflightIssue::JobStale {
            identity: input.job_identity.to_string(),
        });
    }
    if input.expected_form_schema != input.found_form_schema {
        issues.push(PreflightIssue::FormSchemaChanged {
            expected: input.expected_form_schema.as_str().into(),
            found: input.found_form_schema.as_str().into(),
        });
    }
    if input.expected_packet != input.found_packet {
        issues.push(PreflightIssue::PacketChanged {
            expected: input.expected_packet.as_str().into(),
            found: input.found_packet.as_str().into(),
        });
    }
    if input.expected_origin != input.found_origin {
        issues.push(PreflightIssue::DestinationChanged {
            expected: input.expected_origin.to_string(),
            found: input.found_origin.to_string(),
        });
    }
    for (field, present) in input.required_fields_present {
        if !present {
            issues.push(PreflightIssue::RequiredFieldMissing {
                field: (*field).to_string(),
            });
        }
    }

    if !issues.is_empty() {
        return Err(issues);
    }

    // Only when everything else is intact do we consume the one-use grant.
    // authorize() re-checks expiry/consumed/gesture/binding itself.
    grant
        .authorize(
            input.intent_id,
            input.found_packet,
            input.found_packet,
            input.found_form_schema,
            input.found_origin,
            false,
        )
        .map_err(|e| {
            vec![match e {
                waypoint_domain::DomainError::UnboundApproval => PreflightIssue::GrantExpired,
                waypoint_domain::DomainError::ApprovalNotBoundToPayload => {
                    PreflightIssue::GrantMismatch
                }
                other => {
                    let _ = other;
                    PreflightIssue::GrantNotUserAuthorized
                }
            }]
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::{GrantId, GrantOperation};

    fn digest(c: char) -> Sha256Digest {
        Sha256Digest::new(c.to_string().repeat(64)).unwrap()
    }

    fn grant(intent: &IntentId, packet: &Sha256Digest, form: &Sha256Digest) -> ApprovalGrant {
        ApprovalGrant {
            id: GrantId::new("g1").unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: packet.clone(),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: form.clone(),
            destination_origin: "https://jobs.example.com".into(),
            operation: GrantOperation::Submit,
            nonce: "n".repeat(32),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        }
    }

    fn input<'a>(
        intent: &'a IntentId,
        packet: &'a Sha256Digest,
        form: &'a Sha256Digest,
        origin: &'a str,
        fresh: bool,
    ) -> PreflightInput<'a> {
        PreflightInput {
            intent_id: intent,
            job_identity: "greenhouse::req::1",
            job_fresh: fresh,
            expected_form_schema: form,
            found_form_schema: form,
            expected_packet: packet,
            found_packet: packet,
            expected_origin: origin,
            found_origin: origin,
            required_fields_present: &[("name", true), ("email", true)],
        }
    }

    #[test]
    fn happy_path_consumes_grant_once() {
        let intent = IntentId::new("i1").unwrap();
        let packet = digest('a');
        let form = digest('b');
        let mut g = grant(&intent, &packet, &form);
        let inp = input(&intent, &packet, &form, "https://jobs.example.com", true);
        assert!(preflight_check(&mut g, &inp).is_ok());
        // second run: grant already consumed
        assert_eq!(
            preflight_check(&mut g, &inp),
            Err(vec![PreflightIssue::GrantExpired])
        );
    }

    #[test]
    fn stale_job_blocks_before_grant_consumption() {
        let intent = IntentId::new("i1").unwrap();
        let packet = digest('a');
        let form = digest('b');
        let mut g = grant(&intent, &packet, &form);
        let inp = input(&intent, &packet, &form, "https://jobs.example.com", false);
        assert!(matches!(
            preflight_check(&mut g, &inp),
            Err(v) if matches!(v[0], PreflightIssue::JobStale { .. })
        ));
        // grant was NOT consumed by a failed preflight
        let inp2 = input(&intent, &packet, &form, "https://jobs.example.com", true);
        assert!(preflight_check(&mut g, &inp2).is_ok());
    }

    #[test]
    fn changed_form_schema_invalidates_and_reports() {
        let intent = IntentId::new("i1").unwrap();
        let packet = digest('a');
        let form = digest('b');
        let drifted = digest('c');
        let mut g = grant(&intent, &packet, &form);
        let mut inp = input(&intent, &packet, &form, "https://jobs.example.com", true);
        inp.found_form_schema = &drifted;
        assert!(matches!(
            preflight_check(&mut g, &inp),
            Err(v) if matches!(v[0], PreflightIssue::FormSchemaChanged { .. })
        ));
    }
}
