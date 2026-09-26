//! T038 — relationship CRM. Records how a relationship is known, which
//! contact routes are public or candidate-supplied, conversation context and
//! follow-up consent. Recommends specific, respectful followups; never
//! invents addresses or bulk outreach.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactRoute {
    PublicProfile,
    CandidateSupplied,
    /// Addresses guessed or scraped without permission are never usable.
    InventedOrUnpermitted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub employer: String,
    pub route: ContactRoute,
    pub how_known: String,
    pub followup_consent: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FollowupProposal {
    pub contact_id: String,
    pub message_draft: String,
    pub tone: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrmError {
    RouteNotPermitted(ContactRoute),
    NoFollowupConsent,
}

pub fn propose_followup(contact: &Contact, context: &str) -> Result<FollowupProposal, CrmError> {
    if matches!(contact.route, ContactRoute::InventedOrUnpermitted) {
        return Err(CrmError::RouteNotPermitted(contact.route));
    }
    if !contact.followup_consent {
        return Err(CrmError::NoFollowupConsent);
    }
    Ok(FollowupProposal {
        contact_id: contact.id.clone(),
        message_draft: format!(
            "Hi {}, given {} — would you be open to a short conversation? \
             No worries if the timing is wrong.",
            contact.name, context
        ),
        tone: "respectful, specific, single recipient",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(route: ContactRoute, consent: bool) -> Contact {
        Contact {
            id: "c1".into(),
            name: "Dana".into(),
            employer: "Acme".into(),
            route,
            how_known: "former colleague".into(),
            followup_consent: consent,
        }
    }

    #[test]
    fn permitted_route_with_consent_proposes_respectful_message() {
        let p = propose_followup(
            &contact(ContactRoute::CandidateSupplied, true),
            "we worked on the payments migration",
        )
        .unwrap();
        assert!(p.message_draft.contains("Dana"));
        assert!(p.message_draft.contains("payments migration"));
        assert!(!p.message_draft.contains("Dear Sir or Madam"));
    }

    #[test]
    fn invented_addresses_are_never_contactable() {
        assert!(matches!(
            propose_followup(&contact(ContactRoute::InventedOrUnpermitted, true), "x"),
            Err(CrmError::RouteNotPermitted(_))
        ));
    }

    #[test]
    fn no_consent_no_proposal() {
        assert_eq!(
            propose_followup(&contact(ContactRoute::PublicProfile, false), "x"),
            Err(CrmError::NoFollowupConsent)
        );
    }
}
