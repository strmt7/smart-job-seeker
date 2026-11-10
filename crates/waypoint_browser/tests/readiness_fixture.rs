#[path = "support/readiness.rs"]
mod readiness;

use std::io::{self, Write};

#[test]
fn readiness_marker_is_invisible_until_every_byte_is_written() {
    let folder = tempfile::tempdir().unwrap();
    let destination = folder.path().join("ready");
    readiness::publish(&destination, |file| {
        assert!(!destination.exists());
        file.write_all(b"fictional-")?;
        file.flush()?;
        assert!(!destination.exists(), "partial marker became visible");
        file.write_all(b"profile")?;
        assert!(!destination.exists(), "marker published before commit");
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"fictional-profile");
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
}

#[test]
fn failed_readiness_write_leaves_no_marker_or_staging_file() {
    let folder = tempfile::tempdir().unwrap();
    let destination = folder.path().join("ready");
    let error = readiness::publish(&destination, |file| {
        file.write_all(b"partial")?;
        Err(io::Error::other("fictional interrupted write"))
    })
    .unwrap_err();
    assert!(error.to_string().contains("interrupted write"));
    assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
}

#[test]
fn readiness_publication_never_replaces_an_existing_marker() {
    let folder = tempfile::tempdir().unwrap();
    let destination = folder.path().join("ready");
    std::fs::write(&destination, b"existing fixture").unwrap();
    assert!(readiness::publish(&destination, |file| file.write_all(b"new fixture")).is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), b"existing fixture");
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
}
