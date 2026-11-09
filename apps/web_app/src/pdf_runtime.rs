//! Native bootstrap for an app-private PDF runtime; no host Python or pip needed.
use serde::Deserialize;
use std::{collections::BTreeSet, io::Cursor, path::Path};
use waypoint_domain::sha256_hex;

const CATALOG: &str = include_str!("../windows-runtime-artifacts.json");

#[derive(Deserialize)]
struct Catalog {
    version: u32,
    platform: String,
    python: Vec<Artifact>,
}
#[derive(Deserialize)]
struct Artifact {
    id: String,
    url: String,
    bytes: usize,
    sha256: String,
    archive_prefix: String,
    destination: String,
    max_unpacked_bytes: u64,
}

fn catalog() -> Result<Catalog, String> {
    let catalog: Catalog = serde_json::from_str(CATALOG).map_err(|e| e.to_string())?;
    if catalog.version != 1 || catalog.platform != "windows-x86_64" || catalog.python.len() != 4 {
        return Err("PDF runtime catalog is incompatible".into());
    }
    Ok(catalog)
}

fn checked_path(value: &str, seen: &mut BTreeSet<String>) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_control() || "\\:*?\"<>|".contains(c))
        || value.split('/').any(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            part.is_empty()
                || matches!(part, "." | "..")
                || part.ends_with(['.', ' '])
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
        || !seen.insert(value.to_lowercase())
    {
        return Err("PDF runtime archive has an invalid or duplicate Windows path".into());
    }
    Ok(())
}

fn extract(
    root: &Path,
    artifact: &Artifact,
    bytes: &[u8],
    seen: &mut BTreeSet<String>,
) -> Result<(), String> {
    if bytes.len() != artifact.bytes || sha256_hex(bytes) != artifact.sha256 {
        return Err(format!(
            "{} does not match its pinned size and SHA-256",
            artifact.id
        ));
    }
    if artifact.max_unpacked_bytes > 150 * 1024 * 1024 {
        return Err("PDF runtime extraction budget is invalid".into());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > 10000 {
        return Err("PDF runtime archive contains too many entries".into());
    }
    let mut total = 0_u64;
    let mut members = Vec::new();
    // Validate the whole selected inventory before creating executable files.
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?;
        let Some(relative) = name.strip_prefix(&artifact.archive_prefix) else {
            continue; // NuGet metadata is outside tools/, never executed or extracted.
        };
        if entry.is_dir() {
            if !relative.is_empty() {
                checked_path(relative.trim_end_matches('/'), &mut BTreeSet::new())?;
            }
            continue;
        }
        if entry.encrypted()
            || entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000))
        {
            return Err("PDF runtime archive contains an unsupported entry".into());
        }
        let name = format!("{}{relative}", artifact.destination);
        checked_path(&name, seen)?;
        if relative.contains(".data/") {
            return Err("PDF wheel requires an unsupported installation scheme".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("PDF extraction size overflow")?;
        if total > artifact.max_unpacked_bytes {
            return Err("PDF runtime contents exceed the extraction budget".into());
        }
        members.push((index, name, entry.size()));
    }
    if members.is_empty() {
        return Err("PDF runtime archive is empty".into());
    }
    for (index, relative, expected_size) in members {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let destination = root.join(relative);
        std::fs::create_dir_all(destination.parent().ok_or("Invalid runtime directory")?)
            .map_err(|e| format!("Cannot create PDF runtime folder: {e}"))?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| format!("Cannot write PDF runtime file: {e}"))?;
        let written = std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
        if written != expected_size {
            return Err("PDF runtime archive entry has an unexpected size".into());
        }
    }
    Ok(())
}

/// Stage, check and activate a complete runtime, preserving any earlier one on failure.
pub fn install(
    control: &Path,
    id: &str,
    verify: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<String, String> {
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (control, id, verify);
        Err("Automatic PDF runtime installation supports Windows x64".into())
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    install_with(
        control,
        id,
        &catalog()?.python,
        |artifact| {
            if artifact.bytes > 32 * 1024 * 1024 {
                return Err("PDF runtime download exceeds its memory budget".into());
            }
            // The net cap includes HTTP headers and chunk framing; the decoded
            // body still has to equal the catalog size and digest exactly.
            let response =
                waypoint_net::get_capped(&artifact.url, 60_000, artifact.bytes + 1024 * 1024)
                    .map_err(|e| {
                        format!(
                        "Could not download {}: {e}. Retry setup when connectivity is restored.",
                        artifact.id
                    )
                    })?;
            if response.status != 200 {
                return Err(format!(
                    "{} download returned HTTP {}",
                    artifact.id, response.status
                ));
            }
            Ok(response.body)
        },
        verify,
    )
}

fn install_with(
    control: &Path,
    id: &str,
    artifacts: &[Artifact],
    mut download: impl FnMut(&Artifact) -> Result<Vec<u8>, String>,
    verify: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<String, String> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || !control.is_absolute() {
        return Err("Invalid PDF runtime installation destination".into());
    }
    let root = control.join("pdf-runtimes");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let target = root.join(id);
    if target.exists() {
        return Err("This PDF runtime destination already exists".into());
    }
    let stage = tempfile::Builder::new()
        .prefix(".setup-")
        .tempdir_in(&root)
        .map_err(|e| e.to_string())?;
    let mut seen = BTreeSet::new();
    for artifact in artifacts {
        let bytes = download(artifact)?;
        extract(stage.path(), artifact, &bytes, &mut seen)?;
    }
    for name in [
        "python.exe",
        "LICENSE.txt",
        "Lib/site-packages/pypdf/__init__.py",
        "Lib/site-packages/pypdfium2/__init__.py",
        "Lib/site-packages/PIL/__init__.py",
    ] {
        if !stage.path().join(name).is_file() {
            return Err(format!("PDF runtime is missing {name}"));
        }
    }
    verify(&stage.path().join("python.exe"))?;
    // No running processes use the staging folder when it is moved.
    std::fs::rename(stage.path(), &target)
        .map_err(|e| format!("Cannot publish PDF runtime: {e}"))?;
    crate::pdf_setup::activate(control, id, "portable")?;
    Ok(target.join("python.exe").display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture(entries: &[(&str, &[u8])]) -> (Artifact, Vec<u8>) {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        (
            Artifact {
                id: "fixture".into(),
                url: "https://invalid.example/fixture".into(),
                bytes: bytes.len(),
                sha256: sha256_hex(&bytes),
                archive_prefix: String::new(),
                destination: String::new(),
                max_unpacked_bytes: 150 * 1024 * 1024,
            },
            bytes,
        )
    }

    #[test]
    fn refuses_corruption_traversal_windows_aliases_and_expansion_before_activation() {
        let root = tempfile::tempdir().unwrap();
        for entries in [
            vec![("../outside", &b"bad"[..])],
            vec![("CON.txt", &b"bad"[..])],
            vec![("a", &b"one"[..]), ("A", &b"two"[..])],
            vec![("a:b", &b"bad"[..])],
        ] {
            let (artifact, bytes) = fixture(&entries);
            assert!(extract(root.path(), &artifact, &bytes, &mut BTreeSet::new()).is_err());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
        let (mut artifact, mut bytes) = fixture(&[("file", b"content")]);
        bytes[0] ^= 1;
        assert!(extract(root.path(), &artifact, &bytes, &mut BTreeSet::new()).is_err());
        bytes[0] ^= 1;
        artifact.max_unpacked_bytes = 2;
        assert!(extract(root.path(), &artifact, &bytes, &mut BTreeSet::new()).is_err());
    }

    #[test]
    fn failed_native_setup_preserves_active_runtime_and_cleans_its_own_stage() {
        let control = tempfile::tempdir().unwrap();
        std::fs::write(control.path().join("pdf-runtime.json"), b"previous runtime").unwrap();
        let (artifact, bytes) = fixture(&[
            ("python.exe", b"not executable"),
            ("LICENSE.txt", b"notice"),
            ("Lib/site-packages/pypdf/__init__.py", b""),
            ("Lib/site-packages/pypdfium2/__init__.py", b""),
            ("Lib/site-packages/PIL/__init__.py", b""),
        ]);
        let result = install_with(
            control.path(),
            "0123456789abcdef0123456789abcdef",
            &[artifact],
            |_| Ok(bytes.clone()),
            |_| Err("fictional import failure".into()),
        );
        assert_eq!(result.unwrap_err(), "fictional import failure");
        assert_eq!(
            std::fs::read(control.path().join("pdf-runtime.json")).unwrap(),
            b"previous runtime"
        );
        assert_eq!(
            std::fs::read_dir(control.path().join("pdf-runtimes"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn native_setup_activates_only_after_verification_without_a_host_python() {
        let control = tempfile::tempdir().unwrap();
        let (artifact, bytes) = fixture(&[
            ("python.exe", b"fictional"),
            ("LICENSE.txt", b"notice"),
            ("Lib/site-packages/pypdf/__init__.py", b""),
            ("Lib/site-packages/pypdfium2/__init__.py", b""),
            ("Lib/site-packages/PIL/__init__.py", b""),
        ]);
        let result = install_with(
            control.path(),
            "0123456789abcdef0123456789abcdef",
            &[artifact],
            |_| Ok(bytes.clone()),
            |path| {
                assert!(path.is_file());
                assert!(!control.path().join("pdf-runtime.json").exists());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            crate::pdf_setup::managed_python(control.path()).unwrap(),
            std::path::PathBuf::from(result)
        );
        assert_eq!(catalog().unwrap().python.len(), 4);
    }
}
