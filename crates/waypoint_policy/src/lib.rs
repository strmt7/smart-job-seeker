//! Waypoint policy layer (task T002 code, gate G0).
//!
//! Enforces the source permission registry: no external write may be planned
//! against a source whose registry entry does not explicitly allow that
//! operation for the candidate route.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    AllowedPublic,
    AllowedAuthenticated,
    RequiresReview,
    Forbidden,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Discover,
    Read,
    Prefill,
    Upload,
    Submit,
    Confirm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePermission {
    pub source: String,
    pub discover: Capability,
    pub read: Capability,
    pub prefill: Capability,
    pub upload: Capability,
    pub submit: Capability,
    pub confirm: Capability,
}

impl SourcePermission {
    pub fn capability_for(&self, op: Operation) -> Capability {
        match op {
            Operation::Discover => self.discover,
            Operation::Read => self.read,
            Operation::Prefill => self.prefill,
            Operation::Upload => self.upload,
            Operation::Submit => self.submit,
            Operation::Confirm => self.confirm,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// Operation forbidden for this source (e.g. LinkedIn automation).
    Forbidden {
        source: String,
        op: Operation,
    },
    /// Operation needs a recorded per-site review before use.
    RequiresReview {
        source: String,
        op: Operation,
    },
    /// Employer-side authenticated endpoint cannot be selected as a
    /// candidate public submit/read route.
    NotAPublicCandidateRoute {
        source: String,
        op: Operation,
    },
    UnknownSource(String),
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::Forbidden { source, op } => {
                write!(f, "source '{source}' forbids operation {op:?}")
            }
            PolicyError::RequiresReview { source, op } => write!(
                f,
                "source '{source}' requires a recorded review before {op:?}"
            ),
            PolicyError::NotAPublicCandidateRoute { source, op } => write!(
                f,
                "source '{source}' offers {op:?} only through employer credentials"
            ),
            PolicyError::UnknownSource(s) => write!(f, "no permission record for source '{s}'"),
        }
    }
}

impl std::error::Error for PolicyError {}

#[derive(Debug, Default)]
pub struct PermissionRegistry {
    entries: Vec<SourcePermission>,
}

impl PermissionRegistry {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        Ok(Self {
            entries: serde_json::from_str(json)?,
        })
    }

    pub fn lookup(&self, source: &str) -> Option<&SourcePermission> {
        self.entries.iter().find(|e| e.source == source)
    }

    /// May the app plan a *candidate* public (unauthenticated) route for `op`
    /// against `source`?
    pub fn check_public_route(&self, source: &str, op: Operation) -> Result<(), PolicyError> {
        let entry = self
            .lookup(source)
            .ok_or_else(|| PolicyError::UnknownSource(source.to_string()))?;
        match entry.capability_for(op) {
            Capability::AllowedPublic => Ok(()),
            Capability::NotApplicable => Err(PolicyError::Forbidden {
                source: source.into(),
                op,
            }),
            Capability::Forbidden => Err(PolicyError::Forbidden {
                source: source.into(),
                op,
            }),
            Capability::RequiresReview => Err(PolicyError::RequiresReview {
                source: source.into(),
                op,
            }),
            Capability::AllowedAuthenticated => Err(PolicyError::NotAPublicCandidateRoute {
                source: source.into(),
                op,
            }),
        }
    }

    /// May the app plan `op` for `source` when the operation has a recorded
    /// review (for RequiresReview rows) and the route is allowed at all?
    pub fn check_with_review(
        &self,
        source: &str,
        op: Operation,
        review_recorded: bool,
    ) -> Result<(), PolicyError> {
        match self.check_public_route(source, op) {
            Err(PolicyError::RequiresReview { source, op }) if review_recorded => Ok(()),
            other => other,
        }
    }
}

/// The initial registry, mirroring docs/g0/SOURCE_PERMISSION_REGISTRY.md.
pub const INITIAL_REGISTRY_JSON: &str = include_str!("../data/source_permissions.json");

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> PermissionRegistry {
        PermissionRegistry::from_json(INITIAL_REGISTRY_JSON).unwrap()
    }

    #[test]
    fn registry_loads() {
        let r = registry();
        assert!(r.lookup("greenhouse").is_some());
        assert!(r.lookup("linkedin").is_some());
    }

    #[test]
    fn employer_credential_endpoint_is_not_a_public_submit_route() {
        let r = registry();
        // Model case: a source whose submit exists only for employers
        assert_eq!(
            r.check_public_route("workday", Operation::Submit),
            Err(PolicyError::RequiresReview {
                source: "workday".into(),
                op: Operation::Submit
            })
        );
    }

    #[test]
    fn linkedin_automation_is_forbidden() {
        let r = registry();
        assert_eq!(
            r.check_public_route("linkedin", Operation::Read),
            Err(PolicyError::Forbidden {
                source: "linkedin".into(),
                op: Operation::Read
            })
        );
        assert_eq!(
            r.check_public_route("linkedin", Operation::Submit),
            Err(PolicyError::Forbidden {
                source: "linkedin".into(),
                op: Operation::Submit
            })
        );
        // Forbidden stays forbidden even with a review recorded.
        assert_eq!(
            r.check_with_review("linkedin", Operation::Submit, true),
            Err(PolicyError::Forbidden {
                source: "linkedin".into(),
                op: Operation::Submit
            })
        );
    }

    #[test]
    fn public_ats_discovery_allowed_but_prefill_needs_review() {
        let r = registry();
        assert!(r
            .check_public_route("greenhouse", Operation::Discover)
            .is_ok());
        assert!(r.check_public_route("lever", Operation::Discover).is_ok());
        assert!(matches!(
            r.check_public_route("ashby", Operation::Prefill),
            Err(PolicyError::RequiresReview { .. })
        ));
        assert!(r
            .check_with_review("ashby", Operation::Prefill, true)
            .is_ok());
    }

    #[test]
    fn unknown_source_is_rejected_not_assumed_allowed() {
        let r = registry();
        assert!(matches!(
            r.check_public_route("some_random_board", Operation::Submit),
            Err(PolicyError::UnknownSource(_))
        ));
    }
}
