//! T023 — bounded drafting loop.
//!
//! The model proposes tree edits; it never gets raw document authority.
//! Two repair attempts maximum, then the unresolved issue is surfaced
//! (§7: no endless "thinking" spinner). Character limits follow the
//! portal's actual counting rule where known.

use waypoint_inference::{Inference, InferenceError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharCountRule {
    UnicodeCodePoints,
    Utf16Units,
    Bytes,
}

pub fn count_chars(text: &str, rule: CharCountRule) -> usize {
    match rule {
        CharCountRule::UnicodeCodePoints => text.chars().count(),
        CharCountRule::Utf16Units => text.encode_utf16().count(),
        CharCountRule::Bytes => text.len(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    /// Deterministic validation failed twice; surface the issue instead of
    /// looping (never an endless spinner).
    UnresolvedAfterRepairs {
        attempts: u8,
        last_issue: String,
    },
    BudgetExceeded,
    Inference(InferenceError),
}

impl From<InferenceError> for DraftError {
    fn from(e: InferenceError) -> Self {
        DraftError::Inference(e)
    }
}

/// A proposed edit as typed data: target span, new text, claims cited.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProposedEdit {
    pub target_node_id: String,
    pub new_text: String,
    pub claim_ids: Vec<String>,
}

/// Deterministic post-generation validation of a single edit.
pub fn validate_edit(
    edit: &ProposedEdit,
    limit: Option<(usize, CharCountRule)>,
) -> Result<(), String> {
    if edit.new_text.trim().is_empty() {
        return Err("empty text".into());
    }
    if let Some((max, rule)) = limit {
        let n = count_chars(&edit.new_text, rule);
        if n > max {
            return Err(format!("length {n} exceeds limit {max} ({rule:?})"));
        }
    }
    Ok(())
}

/// Run the draft loop: generate -> validate -> (repair <= 2) -> resolve or
/// surface the issue. The model sees only the bounded prompt we build.
pub fn draft_with_repairs<I, F>(
    inference: &mut I,
    mut generate: F,
    limit: Option<(usize, CharCountRule)>,
) -> Result<ProposedEdit, DraftError>
where
    I: Inference,
    F: FnMut(&mut I, &str) -> Result<ProposedEdit, InferenceError>,
{
    const MAX_REPAIRS: u8 = 2;
    let mut last_issue = String::new();
    for attempt in 0..=MAX_REPAIRS {
        let prompt = if attempt == 0 {
            "draft".to_string()
        } else {
            format!("repair previous draft: {last_issue}")
        };
        let (usage, _guard) = (0u64, ());
        let _ = usage;
        let edit = generate(inference, &prompt)?;
        match validate_edit(&edit, limit) {
            Ok(()) => return Ok(edit),
            Err(issue) => {
                last_issue = issue;
                if attempt == MAX_REPAIRS {
                    return Err(DraftError::UnresolvedAfterRepairs {
                        attempts: attempt,
                        last_issue,
                    });
                }
            }
        }
    }
    unreachable!("loop returns inside")
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_inference::StubInference;

    fn gen_valid(_: &mut StubInference, _: &str) -> Result<ProposedEdit, InferenceError> {
        Ok(ProposedEdit {
            target_node_id: "n1".into(),
            new_text: "Led the platform team".into(),
            claim_ids: vec![],
        })
    }

    #[test]
    fn valid_draft_passes_first_time() {
        let mut s = StubInference::qualified();
        let edit = draft_with_repairs(
            &mut s,
            gen_valid,
            Some((500, CharCountRule::UnicodeCodePoints)),
        )
        .unwrap();
        assert_eq!(edit.new_text, "Led the platform team");
    }

    #[test]
    fn repairs_stop_after_two_and_surface_issue() {
        let mut s = StubInference::qualified();
        let mut calls = 0;
        let result = draft_with_repairs(
            &mut s,
            |_, _| {
                calls += 1;
                Ok(ProposedEdit {
                    target_node_id: "n1".into(),
                    new_text: "x".repeat(999), // always over the limit
                    claim_ids: vec![],
                })
            },
            Some((10, CharCountRule::UnicodeCodePoints)),
        );
        assert_eq!(calls, 3, "initial + 2 repairs");
        assert!(matches!(
            result,
            Err(DraftError::UnresolvedAfterRepairs { attempts: 2, .. })
        ));
    }

    #[test]
    fn char_count_rules_differ() {
        let text = "héllo 😀";
        assert_eq!(count_chars(text, CharCountRule::UnicodeCodePoints), 7);
        assert_eq!(count_chars(text, CharCountRule::Utf16Units), 8);
        assert!(count_chars(text, CharCountRule::Bytes) > 8);
    }
}
