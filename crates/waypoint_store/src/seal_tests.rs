//! Sealed-at-rest tests.
//!
//! These assert the property that matters, on a real file: the candidate's own
//! words and prepared documents must not be readable in the database file.

use std::sync::Arc;

use waypoint_domain::{ApplicationState, ClaimStatus, JobIdentityId, SourceKind};
use waypoint_seal::{is_sealed, platform_sealer, PlaintextSealer, Sealer, SealerKind};

use crate::work::StoredClaim;
use crate::{JournalRow, Store, StoredPacket};

/// A distinctive string that cannot appear by accident.
const SECRET: &str = "Zx9 candidate fact: waived notice period in Basel (confidential)";
const SECRET_PACKET: &str = "Zx9 prepared document body: my resignation reason (confidential)";

fn temp_db(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wp-seal-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{tag}.sqlite3"));
    let _ = std::fs::remove_file(&path);
    path
}

fn file_contains(path: &std::path::Path, needle: &str) -> bool {
    let bytes = std::fs::read(path).expect("read db file");
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn claim(id: &str, text: &str) -> StoredClaim {
    StoredClaim {
        id: id.into(),
        profile_revision: 1,
        text: text.into(),
        status: ClaimStatus::UserAttested,
        source_kind: SourceKind::CandidateAttestation,
        requires_review: false,
        supersedes: None,
    }
}

fn packet(body: &str) -> StoredPacket {
    StoredPacket {
        job_id: "j1".into(),
        packet_sha256: "a".repeat(64),
        doc_json: format!("{{\"body\":\"{body}\"}}"),
        body: body.into(),
        cited_claim_ids: vec!["c1".into()],
        profile_revision: 1,
        created_at: "100".into(),
        invalidated_at: None,
    }
}

/// On hosts without a real sealing backend the absence assertions cannot hold,
/// and pretending otherwise would be worse than skipping.
fn real_sealer() -> Option<Arc<dyn Sealer>> {
    let s = platform_sealer()?;
    if s.kind() == SealerKind::Dpapi {
        Some(s)
    } else {
        None
    }
}

#[test]
fn the_test_can_actually_detect_plaintext_in_the_file() {
    // Control: without sealing, the secret IS in the file. If this fails, the
    // absence assertions below would be meaningless.
    let path = temp_db("control");
    {
        let mut store = Store::open(&path).unwrap();
        store.insert_claim(&claim("c1", SECRET)).unwrap();
    }
    assert!(
        file_contains(&path, SECRET),
        "the control must find plaintext, otherwise the other tests prove nothing"
    );
    assert_eq!(Store::open(&path).unwrap().sealing_kind(), None);
}

#[test]
fn an_existing_plaintext_store_can_be_sealed_and_the_plaintext_disappears() {
    let Some(sealer) = real_sealer() else {
        eprintln!("no platform sealer here; skipping the absence assertion");
        return;
    };
    let path = temp_db("migrate");

    // Written before sealing existed.
    {
        let mut store = Store::open(&path).unwrap();
        store.insert_claim(&claim("c1", SECRET)).unwrap();
    }
    assert!(file_contains(&path, SECRET));

    // Now enable sealing and migrate.
    let mut store = Store::open(&path).unwrap().with_sealer(Arc::clone(&sealer));
    assert_eq!(store.sealing_kind(), Some(SealerKind::Dpapi));
    let migrated = store.seal_existing_rows().unwrap();
    assert_eq!(migrated, 1, "one claim needed migrating");
    assert!(store.has_sealed_material().unwrap());

    // The candidate's data still reads back...
    let claims = store.all_claims().unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].text, SECRET);
    // ...and the raw stored value is an envelope, not the words.
    let raw = store.raw_claim_text("c1").unwrap().unwrap();
    assert!(
        is_sealed(&raw),
        "the stored value must be a sealed envelope"
    );
    drop(store);

    assert!(
        !file_contains(&path, SECRET),
        "after sealing, the plaintext must be gone from the file on disk"
    );
}

#[test]
fn rows_written_after_sealing_is_enabled_are_never_plaintext() {
    let Some(sealer) = real_sealer() else {
        return;
    };
    let path = temp_db("fresh");
    {
        let mut store = Store::open(&path).unwrap().with_sealer(sealer);
        store.insert_claim(&claim("c1", SECRET)).unwrap();
        let read = store.all_claims().unwrap();
        assert_eq!(read[0].text, SECRET, "readable through the sealer");
    }
    assert!(
        !file_contains(&path, SECRET),
        "a claim written with sealing active must never hit the disk in plaintext"
    );
}

#[test]
fn prepared_documents_are_sealed_too() {
    let Some(sealer) = real_sealer() else {
        return;
    };
    let path = temp_db("packets");
    {
        let mut store = Store::open(&path).unwrap().with_sealer(sealer);
        // jobs are required by the packets foreign key
        store.upsert_job(&crate::tests_support::job("j1")).unwrap();
        store.upsert_packet(&packet(SECRET_PACKET)).unwrap();

        let read = store
            .packet_for_job(&JobIdentityId::new("j1").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(read.body, SECRET_PACKET);
        assert!(read.doc_json.contains(SECRET_PACKET));
        // The job's title (public data) stays searchable in plain columns.
        assert!(store.sealing_kind().is_some());
    }
    assert!(!file_contains(&path, SECRET_PACKET));
}

#[test]
fn a_value_sealed_by_another_backend_is_refused_not_silently_misread() {
    let Some(real) = real_sealer() else {
        return;
    };
    let path = temp_db("foreign");
    {
        // Sealed with the pass-through backend (i.e. not really sealed).
        let mut store = Store::open(&path)
            .unwrap()
            .with_sealer(Arc::new(PlaintextSealer));
        store.insert_claim(&claim("c1", SECRET)).unwrap();
        assert_eq!(store.all_claims().unwrap()[0].text, SECRET);
    }

    // Opening the same file with the DPAPI backend must refuse the foreign
    // envelope rather than returning garbage.
    let store = Store::open(&path).unwrap().with_sealer(real);
    let err = store.all_claims().unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("sealed with") || text.contains("backend"),
        "the refusal must name the mismatch: {text}"
    );
}

#[test]
fn sealing_is_recorded_in_metadata_and_survives_reopen() {
    let Some(sealer) = real_sealer() else {
        return;
    };
    let path = temp_db("meta");
    {
        let store = Store::open(&path).unwrap().with_sealer(sealer);
        assert_eq!(store.sealing_kind(), Some(SealerKind::Dpapi));
    }
    // A reopened store starts without a sealer, but the file records what
    // state it is in, so the app can decide (and tell the user) before reading.
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.sealing_kind(), None);
    let mode: String = reopened
        .conn
        .query_row("SELECT value FROM meta WHERE key='sealing'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(mode, "dpapi", "the file records how it was written");
}

#[test]
fn the_journal_and_job_tables_stay_searchable() {
    let Some(sealer) = real_sealer() else {
        return;
    };
    let mut store = Store::open(&temp_db("jobs")).unwrap().with_sealer(sealer);
    store.upsert_job(&crate::tests_support::job("j1")).unwrap();
    store
        .append_journal("j1", ApplicationState::Submitting, "100", "write_started")
        .unwrap();
    // Public/summary data is deliberately not sealed: searching and reporting
    // must keep working without the key.
    assert_eq!(store.job_count().unwrap(), 1);
    let rows: Vec<JournalRow> = store.journal_entries("j1").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].reason, "write_started");
}

#[test]
fn sealing_the_same_store_twice_changes_nothing_the_second_time() {
    let Some(sealer) = real_sealer() else {
        return;
    };
    let path = temp_db("idempotent");
    {
        let mut store = Store::open(&path).unwrap();
        store.insert_claim(&claim("c1", SECRET)).unwrap();
    }
    let mut store = Store::open(&path).unwrap().with_sealer(sealer);
    assert_eq!(store.seal_existing_rows().unwrap(), 1);
    assert_eq!(
        store.seal_existing_rows().unwrap(),
        0,
        "a second pass must be a no-op"
    );
    assert_eq!(store.all_claims().unwrap()[0].text, SECRET);
}
