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
const MAX_BYTES: usize = 96 * 1024;
const MAX_DOCUMENTS: usize = 16;
const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
    schema: u32,
    pub(crate) source_profile_sha256: String,
    pub(crate) contacts: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) documents: Vec<SnapshotDocument>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    Cv,
    CoverLetter,
    Supporting,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentSelection {
    pub(crate) document_id: String,
    pub(crate) role: Role,
    #[serde(default)]
    pub(crate) purpose: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotDocument {
    pub(crate) id: String,
    pub(crate) selection: DocumentSelection,
    pub(crate) filename: String,
    pub(crate) sha256: String,
    size: usize,
}
impl SnapshotDocument {
    pub(crate) fn description(&self) -> String {
        match self.selection.role {
            Role::Cv => "The selected candidate CV/resume PDF".into(),
            Role::CoverLetter => "The selected candidate cover letter PDF".into(),
            Role::Supporting => format!("Selected supporting PDF: {}", self.selection.purpose),
        }
    }
}
fn document_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub(crate) fn validate_selections(selections: &[DocumentSelection]) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    let mut cv = false;
    let mut cover = false;
    if selections.len() > MAX_DOCUMENTS {
        return Err("Select at most 16 PDFs".into());
    }
    for selection in selections {
        if !document_id(&selection.document_id)
            || !ids.insert(&selection.document_id)
            || selection.purpose.chars().count() > 64
            || selection.purpose.chars().any(char::is_control)
            || (selection.role == Role::Supporting && selection.purpose.trim().is_empty())
            || (selection.role != Role::Supporting && !selection.purpose.is_empty())
        {
            return Err(
                "Each selected PDF needs a unique identity and an explicit, supported purpose"
                    .into(),
            );
        }
        let seen = match selection.role {
            Role::Cv => &mut cv,
            Role::CoverLetter => &mut cover,
            Role::Supporting => continue,
        };
        if *seen {
            return Err("Select only one CV and one cover letter".into());
        }
        *seen = true;
    }
    Ok(())
}
fn filename(role: Role, index: usize) -> String {
    match role {
        Role::Cv => "resume.pdf".into(),
        Role::CoverLetter => "cover-letter.pdf".into(),
        Role::Supporting => format!("supporting-{index}.pdf"),
    }
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|_| "Cannot create immutable candidate input")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot retain candidate input".into())
}
fn retain(directory: &Path, snapshot: &Snapshot) -> Result<String, String> {
    let bytes =
        serde_json::to_vec_pretty(snapshot).map_err(|_| "Cannot encode candidate snapshot")?;
    if bytes.len() > MAX_BYTES {
        return Err("Candidate snapshot exceeds its size limit".into());
    }
    write_new(&directory.join(NAME), &bytes)?;
    Ok(waypoint_domain::sha256_hex(&bytes))
}
fn document_root(directory: &Path) -> Result<std::path::PathBuf, String> {
    ordinary(directory, true)?;
    let attempt = directory
        .parent()
        .ok_or("Candidate record is unavailable")?;
    ordinary(attempt, true)?;
    let materials = attempt.join("materials");
    ordinary(&materials, true)?;
    Ok(materials.join("rehearsal-documents"))
}
pub(crate) fn stage_selected(
    ws: &waypoint_workspace::Workspace,
    directory: &Path,
    job: &str,
    raw_profile: Option<&str>,
    selections: &[DocumentSelection],
) -> Result<String, String> {
    ordinary(directory, true)?;
    if !document_id(job) {
        return Err("Invalid candidate job binding".into());
    }
    validate_selections(selections)?;
    if selections.is_empty() {
        return Err("Select candidate PDFs before staging them".into());
    }
    let contacts = match raw_profile {
        Some(raw) if raw.len() <= 256 * 1024 => crate::suggestions::contact_profile_snapshot(
            &serde_json::from_str(raw).map_err(|_| "Saved contact profile is invalid")?,
        )?,
        Some(_) => return Err("Saved profile exceeds the rehearsal input limit".into()),
        None => json!({}),
    };
    let mut total = 0usize;
    let mut infos = Vec::new();
    for selection in selections {
        let info = ws
            .document_info(&selection.document_id)
            .map_err(|_| "Cannot read selected document metadata")?
            .ok_or("A selected document is no longer available")?;
        total = total
            .checked_add(info.size)
            .ok_or("Selected documents exceed their limit")?;
        if info.size == 0
            || info.size > MAX_PDF_BYTES
            || total > MAX_TOTAL_BYTES
            || !info.name.to_ascii_lowercase().ends_with(".pdf")
        {
            return Err(
                "Use PDFs up to 10 MiB each and 64 MiB in total; convert other formats first"
                    .into(),
            );
        }
        infos.push(info);
    }
    let root = document_root(directory)?;
    fs::create_dir(&root).map_err(|_| "Cannot create candidate PDF folder")?;
    let mut documents = Vec::new();
    for (index, (selection, info)) in selections.iter().zip(infos).enumerate() {
        let bytes = ws
            .document_content(&selection.document_id)
            .map_err(|_| "Cannot read selected PDF")?
            .ok_or("A selected PDF is no longer available")?;
        if bytes.len() != info.size || !bytes.starts_with(b"%PDF-") {
            return Err("Selected PDF bytes do not match their metadata or format".into());
        }
        let filename = filename(selection.role, index);
        write_new(&root.join(&filename), &bytes)?;
        documents.push(SnapshotDocument {
            id: format!("document-{index}"),
            selection: selection.clone(),
            filename,
            sha256: waypoint_domain::sha256_hex(&bytes),
            size: bytes.len(),
        });
    }
    retain(
        directory,
        &Snapshot {
            schema: 2,
            source_profile_sha256: raw_profile
                .map(|raw| waypoint_domain::sha256_hex(raw.as_bytes()))
                .unwrap_or_default(),
            contacts,
            job_id: Some(job.into()),
            documents,
        },
    )
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
        job_id: None,
        documents: Vec::new(),
    };
    retain(directory, &snapshot)
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
    let profile_valid = if snapshot.source_profile_sha256.is_empty() {
        snapshot.schema == 2 && snapshot.contacts == json!({})
    } else {
        digest(&snapshot.source_profile_sha256)
            && crate::suggestions::contact_profile_snapshot(&snapshot.contacts)?
                == snapshot.contacts
    };
    if !profile_valid
        || !matches!(snapshot.schema, 1 | 2)
        || (snapshot.schema == 1
            && (bytes.len() > 48 * 1024
                || snapshot.job_id.is_some()
                || !snapshot.documents.is_empty()))
        || (snapshot.schema == 2
            && (snapshot.documents.is_empty()
                || snapshot.job_id.as_deref().is_none_or(|id| !document_id(id))))
    {
        return Err("Candidate snapshot contains unsupported data".into());
    }
    let selections: Vec<_> = snapshot
        .documents
        .iter()
        .map(|doc| doc.selection.clone())
        .collect();
    validate_selections(&selections)?;
    let mut total = 0usize;
    for (index, doc) in snapshot.documents.iter().enumerate() {
        total = total
            .checked_add(doc.size)
            .ok_or("Candidate PDFs exceed their size limit")?;
        if doc.id != format!("document-{index}")
            || doc.filename != filename(doc.selection.role, index)
            || !digest(&doc.sha256)
            || doc.size == 0
            || doc.size > MAX_PDF_BYTES
            || total > MAX_TOTAL_BYTES
        {
            return Err("Invalid candidate document binding".into());
        }
    }
    Ok(snapshot)
}
impl Snapshot {
    pub(crate) fn metadata(&self, binding: &str) -> Value {
        let mut value = json!({"scope":"saved_candidate_contacts","snapshot_sha256":binding,"source_profile_sha256":self.source_profile_sha256,"profile_sha256":waypoint_domain::sha256_hex(self.contacts.to_string().as_bytes()),"documents":"not_selected"});
        if self.schema == 2 {
            value["scope"] = json!("saved_candidate_materials");
            value["contacts_selected"] = json!(!self.source_profile_sha256.is_empty());
            value["documents"] = json!(self.documents);
            value["document_review"] = json!("not_established_by_selection");
        }
        value
    }
    pub(crate) fn attachments(
        &self,
        directory: &Path,
        job: &str,
    ) -> Result<Option<waypoint_browser::rehearsal::RehearsalAttachments>, String> {
        if self.schema == 1 {
            return Ok(None);
        }
        if self.job_id.as_deref() != Some(job) {
            return Err("Candidate PDFs belong to a different job".into());
        }
        let root = document_root(directory)?;
        ordinary(&root, true)?;
        let mut files = std::collections::BTreeMap::new();
        for doc in &self.documents {
            let path = root.join(&doc.filename);
            ordinary(&path, false)?;
            let mut bytes = Vec::new();
            fs::File::open(path)
                .and_then(|file| file.take(MAX_PDF_BYTES as u64 + 1).read_to_end(&mut bytes))
                .map_err(|_| "Cannot verify candidate PDF")?;
            if bytes.len() != doc.size
                || !bytes.starts_with(b"%PDF-")
                || waypoint_domain::sha256_hex(&bytes) != doc.sha256
            {
                return Err("Candidate PDF changed after selection".into());
            }
            files.insert(doc.filename.clone(), doc.sha256.clone());
        }
        Ok(Some(waypoint_browser::rehearsal::RehearsalAttachments {
            root,
            files,
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn selected(id: &str, role: Role) -> DocumentSelection {
        DocumentSelection {
            document_id: id.into(),
            role,
            purpose: String::new(),
        }
    }
    fn folders() -> (tempfile::TempDir, std::path::PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        fs::create_dir(folder.path().join("materials")).unwrap();
        let snapshots = folder.path().join("screenshots");
        fs::create_dir(&snapshots).unwrap();
        (folder, snapshots)
    }
    fn workspace() -> waypoint_workspace::Workspace {
        let ws = waypoint_workspace::Workspace::in_memory().unwrap();
        ws.add_document(
            "cv",
            "Private CV.pdf",
            include_bytes!("rehearsal-sample.pdf"),
            1,
        )
        .unwrap();
        ws.add_document(
            "letter",
            "Letter.pdf",
            include_bytes!("rehearsal-cover-letter.pdf"),
            2,
        )
        .unwrap();
        ws
    }
    #[test]
    fn selected_pdfs_are_immutable_job_bound_and_never_introduce_demo_contacts() {
        let ws = workspace();
        let (_folder, snapshots) = folders();
        let selections = vec![
            selected("cv", Role::Cv),
            selected("letter", Role::CoverLetter),
        ];
        let binding = stage_selected(&ws, &snapshots, "job-one", None, &selections).unwrap();
        let snapshot = load(&snapshots, &binding).unwrap();
        assert_eq!(snapshot.contacts, json!({}));
        assert_eq!(snapshot.metadata(&binding)["contacts_selected"], false);
        assert_eq!(
            snapshot.metadata(&binding)["document_review"],
            "not_established_by_selection"
        );
        let attachments = snapshot
            .attachments(&snapshots, "job-one")
            .unwrap()
            .unwrap();
        assert_eq!(attachments.files.len(), 2);
        assert_ne!(
            attachments.files["resume.pdf"],
            attachments.files["cover-letter.pdf"]
        );
        assert_eq!(
            fs::read(attachments.root.join("resume.pdf")).unwrap(),
            ws.document_content("cv").unwrap().unwrap()
        );
        assert!(snapshot.attachments(&snapshots, "job-two").is_err());
        assert!(stage_selected(&ws, &snapshots, "job-one", None, &selections).is_err());
        fs::write(
            attachments.root.join("resume.pdf"),
            include_bytes!("rehearsal-cover-letter.pdf"),
        )
        .unwrap();
        assert!(snapshot.attachments(&snapshots, "job-one").is_err());
        assert_eq!(
            ws.document_content("cv").unwrap().unwrap(),
            include_bytes!("rehearsal-sample.pdf")
        );
    }
    #[test]
    fn selected_contact_source_and_document_purposes_remain_explicit() {
        let ws = workspace();
        let (_folder, snapshots) = folders();
        let raw = r#"{"first_name":"Noor","email":"noor@example.invalid","custom_instructions":"not a contact"}"#;
        let mut support = selected("letter", Role::Supporting);
        support.purpose = "Professional registration".into();
        let binding = stage_selected(&ws, &snapshots, "job", Some(raw), &[support]).unwrap();
        let snapshot = load(&snapshots, &binding).unwrap();
        assert_eq!(
            snapshot.contacts,
            json!({"first_name":"Noor","email":"noor@example.invalid"})
        );
        assert_eq!(
            snapshot.source_profile_sha256,
            waypoint_domain::sha256_hex(raw.as_bytes())
        );
        assert_eq!(
            snapshot.documents[0].description(),
            "Selected supporting PDF: Professional registration"
        );
        assert_eq!(snapshot.documents[0].filename, "supporting-0.pdf");
        assert_eq!(
            snapshot.metadata(&binding)["scope"],
            "saved_candidate_materials"
        );
        assert_eq!(snapshot.metadata(&binding)["contacts_selected"], true);
    }
    #[test]
    fn unsupported_duplicate_and_ambiguous_document_selections_are_refused() {
        for value in [
            json!({"document_id":"cv","role":"unknown"}),
            json!({"document_id":"cv","role":"cv","path":"x"}),
        ] {
            assert!(serde_json::from_value::<DocumentSelection>(value).is_err());
        }
        for selections in [
            vec![selected("../cv", Role::Cv)],
            vec![selected("cv", Role::Supporting)],
            vec![selected("cv", Role::Cv), selected("cv", Role::CoverLetter)],
            vec![selected("cv", Role::Cv), selected("letter", Role::Cv)],
            (0..17)
                .map(|n| DocumentSelection {
                    document_id: format!("id-{n}"),
                    role: Role::Supporting,
                    purpose: "Certificate".into(),
                })
                .collect(),
            vec![DocumentSelection {
                document_id: "cv".into(),
                role: Role::Supporting,
                purpose: "x".repeat(65),
            }],
            vec![DocumentSelection {
                document_id: "cv".into(),
                role: Role::Supporting,
                purpose: "a\nb".into(),
            }],
        ] {
            assert!(validate_selections(&selections).is_err());
        }
    }
    #[test]
    fn missing_wrong_format_and_oversized_documents_never_become_candidate_inputs() {
        let ws = workspace();
        ws.add_document("docx", "Template.docx", b"PK", 1).unwrap();
        ws.add_document("invalid", "Broken.pdf", b"not a PDF", 1)
            .unwrap();
        let mut large = vec![b'x'; MAX_PDF_BYTES + 1];
        large[..5].copy_from_slice(b"%PDF-");
        ws.add_document("large", "Large.pdf", &large, 1).unwrap();
        drop(large);
        for id in ["missing", "docx", "invalid", "large"] {
            let (_folder, snapshots) = folders();
            assert!(
                stage_selected(&ws, &snapshots, "job", None, &[selected(id, Role::Cv)]).is_err()
            );
            assert!(!snapshots.join(NAME).exists());
        }
    }
    #[test]
    fn hash_bound_catalog_cannot_change_paths_roles_aliases_or_resource_limits() {
        let ws = workspace();
        for (key, value) in [
            ("filename", json!("../resume.pdf")),
            ("id", json!("resume")),
            ("size", json!(MAX_PDF_BYTES + 1)),
            ("sha256", json!("bad")),
        ] {
            let (_folder, snapshots) = folders();
            let _ =
                stage_selected(&ws, &snapshots, "job", None, &[selected("cv", Role::Cv)]).unwrap();
            let path = snapshots.join(NAME);
            let mut data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            data["documents"][0][key] = value;
            let bytes = serde_json::to_vec(&data).unwrap();
            fs::write(path, &bytes).unwrap();
            assert!(load(&snapshots, &waypoint_domain::sha256_hex(&bytes)).is_err());
        }
    }
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
