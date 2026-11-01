//! Reviewed, structured form experiences. These are observations for review,
//! never executable instructions, answers, selectors, or approvals.
use super::{Workspace, WorkspaceError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    LocateForm,
    EnterInformation,
    AttachDocuments,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Text,
    Choice,
    Upload,
    Navigation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    AmbiguousLabel,
    ValueChanged,
    ValidationError,
    HiddenControl,
    UnexpectedNavigation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    InspectContext,
    VerifyValueAgain,
    ReviewConstraints,
    InspectManually,
    StopAndReview,
}

/// The user's observation, not independent proof of a causal fix.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Helped,
    DidNotHelp,
    #[default]
    Unknown,
}

/// A user-reviewed observation. Free text remains untrusted even after review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experience {
    pub version: u8,
    pub reviewed: bool,
    pub enabled: bool,
    pub observed_at: i64,
    /// Optional public platform identifier, not an employer, URL or selector.
    pub platform: Option<Platform>,
    pub stage: Stage,
    pub control: Control,
    pub observation: Observation,
    pub response: Response,
    #[serde(default)]
    pub outcome: Outcome,
    /// Optional short user note, kept sealed.
    #[serde(default)]
    pub note: Option<String>,
    /// Explicit permission to include this note in local model context.
    #[serde(default)]
    pub use_note_in_model: bool,
}

/// Known categories are weak hints; tenant-specific behavior must still be inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Greenhouse,
    Lever,
    Ashby,
    Workday,
    Workable,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExperienceMatch {
    pub id: String,
    pub revision: u64,
    pub experience: Experience,
    pub same_platform: bool,
}

fn validate(experience: &Experience) -> Result<(), WorkspaceError> {
    if experience.version != 1 || !experience.reviewed || experience.observed_at <= 0 {
        return Err(WorkspaceError::Export(
            "Experience must have a supported version, review and observation time".into(),
        ));
    }
    if experience.note.as_ref().is_some_and(|note| {
        note.trim().is_empty()
            || note.len() > 512
            || note.chars().count() > 280
            || note.chars().any(char::is_control)
    }) {
        return Err(WorkspaceError::Export("Experience note must be 1–280 characters, at most 512 bytes, without control characters".into()));
    }
    if experience.use_note_in_model && experience.note.is_none() {
        return Err(WorkspaceError::Export(
            "A note is required before enabling local model use".into(),
        ));
    }
    Ok(())
}

impl Workspace {
    /// Fresh, bounded inventory for user review. No decrypted cache is retained.
    pub fn form_experiences(&self) -> Result<Vec<ExperienceMatch>, WorkspaceError> {
        let mut entries = Vec::new();
        for id in self.store.form_experience_ids()? {
            let Some((revision, bytes)) = self.store.form_experience(&id)? else {
                continue;
            };
            let experience: Experience = serde_json::from_slice(&bytes)
                .map_err(|e| WorkspaceError::Export(format!("Invalid stored experience: {e}")))?;
            validate(&experience)?;
            entries.push(ExperienceMatch {
                id,
                revision,
                experience,
                same_platform: false,
            });
        }
        Ok(entries)
    }
    /// The caller must obtain an explicit user review before setting `reviewed`.
    /// IDs must be newly generated random 128-bit lowercase hexadecimal strings.
    pub fn save_form_experience(
        &self,
        id: &str,
        expected: u64,
        experience: &Experience,
    ) -> Result<u64, WorkspaceError> {
        validate(experience)?;
        let content =
            serde_json::to_vec(experience).map_err(|e| WorkspaceError::Export(e.to_string()))?;
        Ok(self.store.save_form_experience(id, expected, &content)?)
    }

    pub fn revoke_form_experience(&self, id: &str, expected: u64) -> Result<bool, WorkspaceError> {
        Ok(self.store.delete_form_experience(id, expected)?)
    }

    /// Update only the reported outcome, preserving the original observation time.
    pub fn correct_form_experience_outcome(
        &self,
        id: &str,
        expected: u64,
        outcome: Outcome,
    ) -> Result<u64, WorkspaceError> {
        let mut experience = self.read_experience_at_revision(id, expected)?;
        experience.outcome = outcome;
        self.save_form_experience(id, expected, &experience)
    }

    /// Update or clear a note while preserving the rest of the reviewed record.
    pub fn correct_form_experience_note(
        &self,
        id: &str,
        expected: u64,
        note: Option<String>,
        use_note_in_model: bool,
    ) -> Result<u64, WorkspaceError> {
        let mut experience = self.read_experience_at_revision(id, expected)?;
        experience.note = note;
        experience.use_note_in_model = use_note_in_model;
        self.save_form_experience(id, expected, &experience)
    }

    /// Enable or disable a reviewed experience without changing its content.
    pub fn set_form_experience_enabled(
        &self,
        id: &str,
        expected: u64,
        enabled: bool,
    ) -> Result<u64, WorkspaceError> {
        let mut experience = self.read_experience_at_revision(id, expected)?;
        experience.enabled = enabled;
        self.save_form_experience(id, expected, &experience)
    }

    fn read_experience_at_revision(
        &self,
        id: &str,
        expected: u64,
    ) -> Result<Experience, WorkspaceError> {
        let (revision, bytes) = self
            .store
            .form_experience(id)?
            .ok_or_else(|| WorkspaceError::NotFound("Experience not found".into()))?;
        if revision != expected {
            return Err(WorkspaceError::Export(
                "Experience changed; reload before correcting it".into(),
            ));
        }
        let experience: Experience = serde_json::from_slice(&bytes)
            .map_err(|e| WorkspaceError::Export(format!("Invalid stored experience: {e}")))?;
        validate(&experience)?;
        Ok(experience)
    }

    /// A bounded list of fallible hints, freshly read on every call. A platform
    /// match affects ordering only; no match or a different platform is allowed.
    pub fn related_form_experiences(
        &self,
        stage: Stage,
        control: Control,
        platform: Option<Platform>,
    ) -> Result<Vec<ExperienceMatch>, WorkspaceError> {
        let mut matches = Vec::new();
        for entry in self.form_experiences()? {
            let ExperienceMatch {
                id,
                revision,
                experience,
                ..
            } = entry;
            if !experience.enabled || experience.stage != stage || experience.control != control {
                continue;
            }
            let same_platform = platform.is_some()
                && platform != Some(Platform::Other)
                && platform == experience.platform;
            matches.push(ExperienceMatch {
                id,
                revision,
                experience,
                same_platform,
            });
        }
        matches.sort_by(|a, b| {
            b.same_platform
                .cmp(&a.same_platform)
                .then_with(|| b.experience.observed_at.cmp(&a.experience.observed_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        // Keep distinct observation/response pairs before repeating a pattern,
        // so several similar memories do not crowd out a conflicting response.
        let mut selected: Vec<ExperienceMatch> = Vec::new();
        for item in &matches {
            if !selected.iter().any(|previous| {
                previous.experience.observation == item.experience.observation
                    && previous.experience.response == item.experience.response
                    && previous.experience.outcome == item.experience.outcome
            }) {
                selected.push(item.clone());
                if selected.len() == 5 {
                    return Ok(selected);
                }
            }
        }
        for item in matches {
            if !selected.iter().any(|previous| previous.id == item.id) {
                selected.push(item);
                if selected.len() == 5 {
                    break;
                }
            }
        }
        Ok(selected)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use waypoint_store::Store;
    fn lesson() -> Experience {
        Experience {
            version: 1,
            reviewed: true,
            enabled: true,
            observed_at: 1,
            platform: Some(Platform::Lever),
            stage: Stage::EnterInformation,
            control: Control::Text,
            observation: Observation::ValueChanged,
            response: Response::VerifyValueAgain,
            outcome: Outcome::Unknown,
            note: None,
            use_note_in_model: false,
        }
    }
    #[test]
    fn revisions_revocation_and_matching_are_current() {
        let mut ws = Workspace::in_memory().unwrap();
        ws.store = Store::in_memory()
            .unwrap()
            .with_sealer(waypoint_seal::platform_sealer().unwrap());
        let id = "00000000000000000000000000000001";
        let other = "00000000000000000000000000000002";
        assert!(ws
            .save_form_experience(
                id,
                0,
                &Experience {
                    reviewed: false,
                    ..lesson()
                }
            )
            .is_err());
        assert_eq!(ws.save_form_experience(id, 0, &lesson()).unwrap(), 1);
        let mut second = lesson();
        second.platform = Some(Platform::Ashby);
        second.observed_at = 2;
        second.response = Response::StopAndReview;
        ws.save_form_experience(other, 0, &second).unwrap();
        let found = ws
            .related_form_experiences(
                Stage::EnterInformation,
                Control::Text,
                Some(Platform::Lever),
            )
            .unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, id);
        assert!(!found[1].same_platform);
        assert!(ws
            .related_form_experiences(Stage::Review, Control::Text, Some(Platform::Lever))
            .unwrap()
            .is_empty());
        assert_eq!(ws.set_form_experience_enabled(id, 1, false).unwrap(), 2);
        assert!(ws.set_form_experience_enabled(id, 1, true).is_err());
        assert!(ws.save_form_experience(id, 1, &lesson()).is_err());
        assert_eq!(
            ws.related_form_experiences(
                Stage::EnterInformation,
                Control::Text,
                Some(Platform::Lever)
            )
            .unwrap()
            .len(),
            1
        );
        assert!(ws.revoke_form_experience(other, 1).unwrap());
        assert!(ws
            .related_form_experiences(Stage::EnterInformation, Control::Text, None)
            .unwrap()
            .is_empty());
        assert_eq!(ws.set_form_experience_enabled(id, 2, true).unwrap(), 3);
        let restored = ws
            .related_form_experiences(Stage::EnterInformation, Control::Text, None)
            .unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].experience, lesson());
    }

    #[test]
    fn conflicting_responses_are_not_crowded_out_by_repetitions() {
        let mut ws = Workspace::in_memory().unwrap();
        ws.store = Store::in_memory()
            .unwrap()
            .with_sealer(waypoint_seal::platform_sealer().unwrap());
        for i in 1..=7 {
            let mut item = lesson();
            item.observed_at = i;
            if i == 1 {
                item.response = Response::StopAndReview;
            }
            if i == 2 {
                item.outcome = Outcome::DidNotHelp;
            }
            ws.save_form_experience(&format!("{i:032x}"), 0, &item)
                .unwrap();
        }
        let found = ws
            .related_form_experiences(
                Stage::EnterInformation,
                Control::Text,
                Some(Platform::Lever),
            )
            .unwrap();
        assert_eq!(found.len(), 5);
        assert!(found
            .iter()
            .any(|item| item.experience.response == Response::StopAndReview));
        assert!(found
            .iter()
            .any(|item| item.experience.response == Response::VerifyValueAgain));
        assert!(found
            .iter()
            .any(|item| item.experience.outcome == Outcome::DidNotHelp));
    }

    #[test]
    fn older_records_leave_the_outcome_unknown() {
        let mut value = serde_json::to_value(lesson()).unwrap();
        value.as_object_mut().unwrap().remove("outcome");
        value.as_object_mut().unwrap().remove("note");
        value.as_object_mut().unwrap().remove("use_note_in_model");
        let recovered: Experience = serde_json::from_value(value).unwrap();
        assert_eq!(recovered.outcome, Outcome::Unknown);
        assert_eq!(recovered.note, None);
        assert!(!recovered.use_note_in_model);
    }

    #[test]
    fn context_notes_are_bounded_and_survive_outcome_corrections() {
        let mut ws = Workspace::in_memory().unwrap();
        ws.store = Store::in_memory()
            .unwrap()
            .with_sealer(waypoint_seal::platform_sealer().unwrap());
        let id = "00000000000000000000000000000011";
        let mut item = lesson();
        item.note =
            Some("Choose the visible add-entry control before opening the next section".into());
        ws.save_form_experience(id, 0, &item).unwrap();
        ws.correct_form_experience_outcome(id, 1, Outcome::Helped)
            .unwrap();
        assert_eq!(ws.form_experiences().unwrap()[0].experience.note, item.note);
        for invalid in [
            "x".repeat(281),
            " ".into(),
            "bad\nline".into(),
            "🙂".repeat(129),
        ] {
            item.note = Some(invalid);
            assert!(ws.save_form_experience(id, 2, &item).is_err());
        }
        assert!(ws.correct_form_experience_note(id, 1, None, false).is_err());
        assert!(ws
            .correct_form_experience_note(id, 2, Some(" ".into()), false)
            .is_err());
        assert_eq!(
            ws.correct_form_experience_note(id, 2, Some("Check the label context".into()), true)
                .unwrap(),
            3
        );
        let updated = &ws.form_experiences().unwrap()[0].experience;
        assert_eq!(updated.note.as_deref(), Some("Check the label context"));
        assert!(updated.use_note_in_model);
        assert_eq!(updated.outcome, Outcome::Helped);
        assert_eq!(updated.observed_at, item.observed_at);
        assert!(ws.correct_form_experience_note(id, 3, None, true).is_err());
        assert_eq!(
            ws.correct_form_experience_note(id, 3, None, false).unwrap(),
            4
        );
        assert_eq!(ws.form_experiences().unwrap()[0].experience.note, None);
    }
}
