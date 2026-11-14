//! Explicit contact snapshots for the network-isolated saved-page actor.
//! Public capture requests contain only the digest, never candidate values.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
const NAME: &str = "rehearsal-candidate.json";
const MAX_BYTES: usize = 48 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
    schema: u32,
    pub(crate) source_profile_sha256: String,
    pub(crate) contacts: Value,
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn ordinary(path: &Path, is_directory: bool) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "Candidate snapshot is unavailable")?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked || (is_directory && !metadata.is_dir()) || (!is_directory && !metadata.is_file()) {
        return Err("Candidate snapshot needs a regular local file and directory".into());
    }
    Ok(())
}
pub(crate) fn stage(directory: &Path, raw_profile: &str) -> Result<String, String> {
    ordinary(directory, true)?;
    if raw_profile.len() > 256 * 1024 {
        return Err("Saved profile exceeds the rehearsal input limit".into());
    }
    let profile: Value =
        serde_json::from_str(raw_profile).map_err(|_| "Saved contact profile is invalid")?;
    let contacts = crate::suggestions::contact_profile_snapshot(&profile)?;
    let snapshot = Snapshot {
        schema: 1,
        source_profile_sha256: waypoint_domain::sha256_hex(raw_profile.as_bytes()),
        contacts,
    };
    let bytes =
        serde_json::to_vec_pretty(&snapshot).map_err(|_| "Cannot encode candidate snapshot")?;
    if bytes.len() > MAX_BYTES {
        return Err("Candidate snapshot exceeds its size limit".into());
    }
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join(NAME))
        .map_err(|_| "Cannot create candidate snapshot")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot retain candidate snapshot")?;
    Ok(waypoint_domain::sha256_hex(&bytes))
}
pub(crate) fn load(directory: &Path, expected: &str) -> Result<Snapshot, String> {
    if !digest(expected) {
        return Err("Invalid candidate snapshot binding".into());
    }
    ordinary(directory, true)?;
    let path = directory.join(NAME);
    ordinary(&path, false)?;
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .and_then(|file| file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|_| "Cannot read candidate snapshot")?;
    if bytes.len() > MAX_BYTES || waypoint_domain::sha256_hex(&bytes) != expected {
        return Err("Candidate snapshot changed or exceeds its limit".into());
    }
    let snapshot: Snapshot =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid candidate snapshot")?;
    if snapshot.schema != 1
        || !digest(&snapshot.source_profile_sha256)
        || crate::suggestions::contact_profile_snapshot(&snapshot.contacts)? != snapshot.contacts
    {
        return Err("Candidate snapshot contains unsupported data".into());
    }
    Ok(snapshot)
}
impl Snapshot {
    pub(crate) fn metadata(&self, binding: &str) -> Value {
        json!({"scope":"saved_candidate_contacts","snapshot_sha256":binding,"source_profile_sha256":self.source_profile_sha256,"profile_sha256":waypoint_domain::sha256_hex(self.contacts.to_string().as_bytes()),"documents":"not_selected"})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contacts_are_explicit_bounded_and_never_replaced_by_demo_values() {
        let root = tempfile::tempdir().unwrap();
        let raw = r#"{"email":"case-two@example.invalid","first_name":"Morgan","country":"France","work_authorization":"yes","custom_instructions":"secret","api_key":"private","phone":12}"#;
        let binding = stage(root.path(), raw).unwrap();
        let snapshot = load(root.path(), &binding).unwrap();
        assert_eq!(
            snapshot.contacts,
            json!({"email":"case-two@example.invalid","first_name":"Morgan","country":"France"})
        );
        assert_eq!(
            snapshot.source_profile_sha256,
            waypoint_domain::sha256_hex(raw.as_bytes())
        );
        assert_eq!(snapshot.metadata(&binding)["documents"], "not_selected");
        assert!(
            stage(root.path(), raw).is_err(),
            "existing immutable input must not be overwritten"
        );
        assert!(load(root.path(), &"0".repeat(64)).is_err());
        assert!(load(root.path(), "../elsewhere").is_err());
        fs::write(root.path().join(NAME), b"changed").unwrap();
        assert!(load(root.path(), &binding).is_err());
        for profile in [
            json!({}),
            json!({"email":" "}),
            json!({"first_name":"x".repeat(2001)}),
            json!({"consent":true}),
            json!([]),
        ] {
            let folder = tempfile::tempdir().unwrap();
            assert!(stage(folder.path(), &profile.to_string()).is_err());
            assert!(!folder.path().join(NAME).exists());
        }
    }
    #[test]
    fn even_hash_bound_inputs_cannot_introduce_extra_fields_or_values() {
        for bytes in [br#"{"schema":1,"source_profile_sha256":"bad","contacts":{"email":"m@example.invalid"}}"#.to_vec(),serde_json::to_vec(&json!({"schema":1,"source_profile_sha256":"a".repeat(64),"contacts":{"email":"m@example.invalid","password":"private"}})).unwrap(),serde_json::to_vec(&json!({"schema":1,"source_profile_sha256":"a".repeat(64),"contacts":{"email":"m@example.invalid"},"instructions":"private"})).unwrap(),vec![b'x';MAX_BYTES+1]] {
            let folder=tempfile::tempdir().unwrap();fs::write(folder.path().join(NAME),&bytes).unwrap();
            assert!(load(folder.path(),&waypoint_domain::sha256_hex(&bytes)).is_err());
        }
    }
}
