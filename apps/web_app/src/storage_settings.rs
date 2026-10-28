//! Per-user pointer to the workspace. Changing it never moves existing data.
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn control_dir() -> Result<PathBuf, String> {
    std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|p| p.join("smart-job-seeker-control"))
        .ok_or_else(|| "User profile directory is unavailable".into())
}

pub fn default_workspace() -> Result<PathBuf, String> {
    std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|p| p.join("smart-job-seeker"))
        .ok_or_else(|| "User profile directory is unavailable".into())
}

fn checked(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
        return Err("Enter an absolute folder path".into());
    }
    let path = PathBuf::from(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Enter an absolute folder path without parent traversal".into());
    }
    if path.exists() && !path.is_dir() {
        return Err("The chosen path is not a folder".into());
    }
    Ok(path)
}

pub fn load_at(control: &Path, default: &Path) -> Result<PathBuf, String> {
    let path = control.join("workspace.json");
    match std::fs::read(&path) {
        Ok(bytes) => {
            if bytes.len() > 8192 {
                return Err("Workspace setting is too large".into());
            }
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| "Workspace setting is invalid")?;
            checked(
                value["data_dir"]
                    .as_str()
                    .ok_or("Workspace setting is missing its folder")?,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(default.to_path_buf()),
        Err(_) => Err("Cannot read workspace setting".into()),
    }
}

pub fn save_at(control: &Path, value: &str) -> Result<PathBuf, String> {
    let path = checked(value)?;
    std::fs::create_dir_all(control).map_err(|_| "Cannot create settings folder")?;
    let mut tmp =
        tempfile::NamedTempFile::new_in(control).map_err(|_| "Cannot write workspace setting")?;
    let data = serde_json::to_vec(&serde_json::json!({"data_dir":path.to_string_lossy()}))
        .map_err(|_| "Cannot encode workspace setting")?;
    tmp.write_all(&data)
        .and_then(|_| tmp.flush())
        .map_err(|_| "Cannot write workspace setting")?;
    tmp.persist(control.join("workspace.json"))
        .map_err(|_| "Cannot save workspace setting")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setting_is_persistent_and_does_not_move_existing_data() {
        let root = tempfile::tempdir().unwrap();
        let control = root.path().join("control");
        let default = root.path().join("default");
        let selected = root.path().join("selected");
        assert_eq!(load_at(&control, &default).unwrap(), default);
        save_at(&control, selected.to_str().unwrap()).unwrap();
        assert_eq!(load_at(&control, &default).unwrap(), selected);
        assert!(!selected.exists());
        assert!(save_at(&control, "relative").is_err());
    }
}
