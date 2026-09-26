//! T037 — calendar coordination. Proposals only; authorized writes need
//! grants; timezone and attendee identity are checked before any write.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedEvent {
    pub title: String,
    /// IANA timezone the invitation states.
    pub timezone: String,
    pub starts_at_utc: String,
    pub duration_minutes: u32,
    pub attendee_email: String,
    pub related_application: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarIssue {
    InvalidTimezone,
    AttendeeDomainMismatch {
        expected_domain: String,
        got: String,
    },
    OverlapsExisting {
        existing_title: String,
    },
    NotAuthorized,
}

pub fn check_conflicts(
    proposal: &ProposedEvent,
    existing: &[(String, String, u32)], // (title, starts_at_utc, duration)
    expected_attendee_domain: &str,
    authorized: bool,
) -> Result<(), Vec<CalendarIssue>> {
    let mut issues = vec![];
    // Timezone must be a plausible IANA name (contains '/').
    if !proposal.timezone.contains('/') {
        issues.push(CalendarIssue::InvalidTimezone);
    }
    let got_domain = proposal
        .attendee_email
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    if got_domain != expected_attendee_domain.to_lowercase() {
        issues.push(CalendarIssue::AttendeeDomainMismatch {
            expected_domain: expected_attendee_domain.into(),
            got: got_domain,
        });
    }
    for (title, start, dur) in existing {
        if *start == proposal.starts_at_utc {
            issues.push(CalendarIssue::OverlapsExisting {
                existing_title: title.clone(),
            });
        }
        let _ = dur;
    }
    if !authorized {
        issues.push(CalendarIssue::NotAuthorized);
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> ProposedEvent {
        ProposedEvent {
            title: "Interview".into(),
            timezone: "Europe/Zurich".into(),
            starts_at_utc: "2026-10-01T09:00:00Z".into(),
            duration_minutes: 60,
            attendee_email: "recruiter@acme.com".into(),
            related_application: "app-1".into(),
        }
    }

    #[test]
    fn clean_authorized_event_passes() {
        assert!(check_conflicts(&event(), &[], "acme.com", true).is_ok());
    }

    #[test]
    fn unauthorized_write_is_refused() {
        assert_eq!(
            check_conflicts(&event(), &[], "acme.com", false),
            Err(vec![CalendarIssue::NotAuthorized])
        );
    }

    #[test]
    fn wrong_attendee_domain_is_refused() {
        let err = check_conflicts(&event(), &[], "beta.com", true).unwrap_err();
        assert!(matches!(
            err[0],
            CalendarIssue::AttendeeDomainMismatch { .. }
        ));
    }

    #[test]
    fn naive_timezone_string_is_refused() {
        let mut e = event();
        e.timezone = "CET".into();
        let err = check_conflicts(&e, &[], "acme.com", true).unwrap_err();
        assert!(err.contains(&CalendarIssue::InvalidTimezone));
    }
}
