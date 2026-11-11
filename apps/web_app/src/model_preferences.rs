//! Persist the user's chosen local vision model independently of candidate facts.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use waypoint_workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Choice {
    pub name: String,
    pub digest: String,
    pub size_bytes: u64,
    pub directory: Option<PathBuf>,
}

impl Choice {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let safe_parts = self.name.rsplit_once(':').is_some_and(|(name, tag)| {
            !tag.is_empty()
                && !matches!(tag, "." | "..")
                && !tag.contains('/')
                && !name.contains(':')
                && name
                    .split('/')
                    .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
        });
        if self.name.is_empty()
            || self.name.len() > 256
            || !safe_parts
            || self
                .name
                .bytes()
                .any(|c| !c.is_ascii_alphanumeric() && !b"._-/:".contains(&c))
            || self.name.to_ascii_lowercase().contains("cloud")
            || self.digest.len() != 64
            || !self.digest.bytes().all(|c| c.is_ascii_hexdigit())
            || !(1024u64.pow(3)..=16 * 1024u64.pow(3)).contains(&self.size_bytes)
            || self.directory.as_ref().is_some_and(|path| {
                !path.is_absolute()
                    || path.to_string_lossy().len() > 1024
                    || path.to_string_lossy().chars().any(char::is_control)
            })
        {
            return Err("Saved model settings are invalid".into());
        }
        #[cfg(windows)]
        if self.directory.as_ref().is_some_and(|path| !matches!(path.components().next(),
            Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))) {
            return Err("Model storage must use a local drive".into());
        }
        Ok(())
    }

    #[cfg(any(windows, test))]
    pub(crate) fn matches(&self, identity: &crate::resident_inference::ResidentIdentity) -> bool {
        self.name == identity.name && self.digest.eq_ignore_ascii_case(&identity.digest)
    }
}

pub(crate) fn read(ws: &Workspace) -> Result<(u64, Option<Choice>), String> {
    let (revision, content) = ws.model_preferences().map_err(|e| e.to_string())?;
    let choice = content
        .map(|bytes| -> Result<Choice, String> {
            let choice: Choice =
                serde_json::from_slice(&bytes).map_err(|_| "Saved model settings are invalid")?;
            choice.validate()?;
            Ok(choice)
        })
        .transpose()?;
    Ok((revision, choice))
}

pub(crate) fn status(ws: &Workspace) -> Result<Value, String> {
    let (revision, choice) = read(ws)?;
    Ok(json!({"revision":revision,"choice":choice}))
}

pub(crate) fn save(ws: &Workspace, body: &Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        revision: u64,
        model: Option<String>,
        directory: Option<PathBuf>,
    }
    let input: Input =
        serde_json::from_value(body.clone()).map_err(|_| "Invalid model settings request")?;
    let choice = if let Some(model) = input.model {
        let addr = "127.0.0.1:11434"
            .parse()
            .map_err(|_| "Invalid local runtime address")?;
        let (code, tags) =
            waypoint_inference::ollama::http_request(addr, "GET", "/api/tags", None, 3000)?;
        if code != 200 {
            return Err("Installed local models could not be read".into());
        }
        let tags: Value =
            serde_json::from_slice(&tags).map_err(|_| "Invalid local model inventory")?;
        let (size_bytes, digest) = crate::model_start::startup_binding(&tags, &model)?;
        let choice = Choice {
            name: model,
            digest: digest.to_ascii_lowercase(),
            size_bytes,
            directory: input.directory,
        };
        choice.validate()?;
        let request =
            serde_json::to_vec(&json!({"model":choice.name})).map_err(|_| "Invalid model name")?;
        let (code, details) = waypoint_inference::ollama::http_request(
            addr,
            "POST",
            "/api/show",
            Some(&request),
            3000,
        )?;
        let details: Value =
            serde_json::from_slice(&details).map_err(|_| "Invalid local model details")?;
        if code != 200
            || !details["capabilities"]
                .as_array()
                .is_some_and(|values| values.iter().any(|v| v == "vision"))
        {
            return Err(
                "Rehearsal requires an installed local model with vision capability".into(),
            );
        }
        Some(choice)
    } else {
        if input.directory.is_some() {
            return Err("Choose a model before setting its folder".into());
        }
        None
    };
    let bytes = choice
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|_| "Cannot prepare model settings")?;
    let revision = ws
        .save_model_preferences(input.revision, bytes.as_deref())
        .map_err(|e| e.to_string())?
        .ok_or("Model settings changed in another window; refresh them before saving")?;
    Ok(json!({"revision":revision,"choice":choice}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choice_rejects_unknown_or_unsafe_identity_and_remote_paths() {
        let good = Choice {
            name: "fictional:vision".into(),
            digest: "a".repeat(64),
            size_bytes: 2 * 1024u64.pow(3),
            directory: None,
        };
        assert!(good.validate().is_ok());
        for name in [
            "",
            "fictional:cloud",
            "fictional\n",
            "fictional;write",
            "../fictional:tag",
            "fictional:../tag",
        ] {
            let mut invalid = good.clone();
            invalid.name = name.into();
            assert!(invalid.validate().is_err());
        }
        let mut invalid = good.clone();
        invalid.directory = Some("relative/models".into());
        assert!(invalid.validate().is_err());
        let matching = crate::resident_inference::ResidentIdentity {
            name: good.name.clone(),
            digest: good.digest.clone(),
        };
        assert!(good.matches(&matching));
        assert!(!good.matches(&crate::resident_inference::ResidentIdentity {
            name: matching.name,
            digest: "b".repeat(64)
        }));
    }
}
