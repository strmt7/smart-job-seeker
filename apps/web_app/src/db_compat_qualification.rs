//! Opt-in, fictional engine-upgrade fixtures. Never opens a user's workspace.
use rusqlite::{params, Connection};
use std::path::PathBuf;
use waypoint_store::Store;

const KEY: [u8; 32] = [81; 32];
const ORIGINAL: &str = "Fictional applicant — München — 資格\nRésumé and certificates";
const UPDATED: &str = "Fictional upgrade verification only";

fn directory() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap();
    let path = PathBuf::from(
        std::env::var_os("SMART_JOBSEEKER_ENGINE_FIXTURE").expect("set private fixture directory"),
    )
    .canonicalize()
    .unwrap();
    assert!(
        path.starts_with(&root) && path != root,
        "fixture must be inside ignored target"
    );
    path
}

fn connection(path: &std::path::Path) -> Connection {
    let conn =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    conn.execute_batch(&format!("PRAGMA key='{}';", "51".repeat(32)))
        .unwrap();
    conn
}

fn fixture_hash(path: &std::path::Path) -> String {
    waypoint_domain::sha256_hex(&std::fs::read(path).unwrap())
}

#[test]
#[ignore = "creates a new fictional database for an explicit engine-upgrade qualification"]
fn capture_engine_compatibility_fixture() {
    let dir = directory();
    let path = dir.join("original.sqlite");
    assert!(!path.exists(), "never overwrite the old-engine fixture");
    drop(Store::open_encrypted(&path, &KEY).unwrap());
    let conn = connection(&path);
    conn.execute_batch(
        "CREATE TABLE engine_compat_fixture(id INTEGER PRIMARY KEY, body TEXT, attachment BLOB);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO engine_compat_fixture VALUES(1, ?1, ?2)",
        params![ORIGINAL, &[0u8, 1, 127, 255][..]],
    )
    .unwrap();
    let cipher: String = conn
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .unwrap();
    let sqlite: String = conn
        .query_row("SELECT sqlite_version()", [], |r| r.get(0))
        .unwrap();
    drop(conn);
    let metadata = serde_json::json!({"sqlite":sqlite,"sqlcipher":cipher,"database_sha256":fixture_hash(&path),"fictional":true});
    std::fs::write(
        dir.join("original.json"),
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .unwrap();
    println!("Original engine fixture: {metadata}");
}

#[test]
#[ignore = "opens a private fictional old-engine fixture, verifies it, then writes a separate upgraded copy"]
fn upgrade_engine_compatibility_fixture() {
    let dir = directory();
    let original = dir.join("original.sqlite");
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("original.json")).unwrap()).unwrap();
    assert_eq!(metadata["fictional"], true);
    assert_eq!(fixture_hash(&original), metadata["database_sha256"]);
    let upgraded = dir.join("upgraded.sqlite");
    assert!(
        !upgraded.exists(),
        "never overwrite a previous qualification"
    );
    std::fs::copy(&original, &upgraded).unwrap();
    assert!(Store::open_encrypted(&upgraded, &[82; 32]).is_err());
    drop(Store::open_encrypted(&upgraded, &KEY).unwrap());
    let conn = connection(&upgraded);
    let body: String = conn
        .query_row(
            "SELECT body FROM engine_compat_fixture WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let attachment: Vec<u8> = conn
        .query_row(
            "SELECT attachment FROM engine_compat_fixture WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(body, ORIGINAL);
    assert_eq!(attachment, [0, 1, 127, 255]);
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    conn.execute(
        "INSERT INTO engine_compat_fixture VALUES(2, ?1, NULL)",
        [UPDATED],
    )
    .unwrap();
    let sqlite: String = conn
        .query_row("SELECT sqlite_version()", [], |r| r.get(0))
        .unwrap();
    let cipher: String = conn
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .unwrap();
    drop(conn);
    assert!(!std::fs::read(&upgraded)
        .unwrap()
        .starts_with(b"SQLite format 3"));
    let reopened = connection(&upgraded);
    assert_eq!(
        reopened
            .query_row(
                "SELECT body FROM engine_compat_fixture WHERE id=2",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        UPDATED
    );
    drop(reopened);
    assert_eq!(fixture_hash(&original), metadata["database_sha256"]);
    println!("Upgraded fictional fixture: SQLite {sqlite}, SQLCipher {cipher}");
}

#[test]
#[ignore = "run the preserved old test executable against the separately upgraded fictional database"]
fn old_reader_engine_compatibility_fixture() {
    let dir = directory();
    let conn = connection(&dir.join("upgraded.sqlite"));
    assert_eq!(
        conn.query_row(
            "SELECT body FROM engine_compat_fixture WHERE id=1",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        ORIGINAL
    );
    assert_eq!(
        conn.query_row(
            "SELECT body FROM engine_compat_fixture WHERE id=2",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        UPDATED
    );
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    let cipher: String = conn
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .unwrap();
    println!("Preserved reader SQLCipher: {cipher}");
}
