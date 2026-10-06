//! Isolated base-tree projection for Cargo metadata without source execution.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Component, Path};

use tempfile::TempDir;
use velnor_actions_mise::GitRequest;

const MAX_PATHS: usize = 100_000;
const MAX_MANIFESTS: usize = 512;
const MAX_METADATA_FILES: usize = 1024;
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOTAL_METADATA_BYTES: usize = 64 * 1024 * 1024;

struct Entry {
    path: String,
    object: String,
}

/// Project every tracked regular path; preserve manifests and lockfile bytes.
///
/// Cargo metadata reads manifests and checks source-path existence, so source
/// files are empty placeholders. Cargo configuration makes the graph unknown:
/// source replacement and patches require separate resolution. The caller must run
/// isolated `cargo metadata --no-deps` with a separate Cargo home. Unsupported
/// entries or resource limits return uncertainty for conservative selection.
pub(crate) fn materialize(root: &Path, base: &str) -> Result<(TempDir, Vec<String>), String> {
    if !object_id(base) {
        return Err("base_tree_invalid_commit".to_owned());
    }
    let args = vec![OsString::from("-r"), OsString::from("-z"), base.into()];
    let output = GitRequest::ls_tree(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    let entries = parse_entries(&output.stdout)?;
    if entries.iter().any(|entry| cargo_config(&entry.path)) {
        return Err("base_graph_cargo_config_requires_resolution".to_owned());
    }
    let manifests: Vec<String> = entries
        .iter()
        .filter(|entry| filename(&entry.path) == Some("Cargo.toml"))
        .map(|entry| entry.path.clone())
        .collect();
    if manifests.len() > MAX_MANIFESTS {
        return Err("base_tree_manifest_cap".to_owned());
    }
    if entries
        .iter()
        .filter(|entry| metadata_file(&entry.path))
        .count()
        > MAX_METADATA_FILES
    {
        return Err("base_tree_metadata_file_cap".to_owned());
    }
    let temporary = tempfile::tempdir().map_err(|err| err.to_string())?;
    let canonical = temporary
        .path()
        .canonicalize()
        .map_err(|err| err.to_string())?;
    let mut metadata_bytes = 0_usize;
    for entry in entries {
        let target = canonical.join(&entry.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let bytes = if metadata_file(&entry.path) {
            read_blob(root, &entry.object)?
        } else {
            Vec::new()
        };
        metadata_bytes = metadata_bytes.saturating_add(bytes.len());
        if metadata_bytes > MAX_TOTAL_METADATA_BYTES {
            return Err("base_tree_metadata_byte_cap".to_owned());
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|err| err.to_string())?;
        file.write_all(&bytes).map_err(|err| err.to_string())?;
    }
    Ok((temporary, manifests))
}

fn parse_entries(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    if !bytes.is_empty() && bytes.last() != Some(&0) {
        return Err("base_tree_unterminated_entry".to_owned());
    }
    let mut entries = Vec::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        if entries.len() >= MAX_PATHS {
            return Err("base_tree_path_cap".to_owned());
        }
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| "base_tree_invalid_entry".to_owned())?;
        let header = std::str::from_utf8(&record[..tab]).map_err(|err| err.to_string())?;
        let path = std::str::from_utf8(&record[tab + 1..]).map_err(|err| err.to_string())?;
        let fields: Vec<&str> = header.split(' ').collect();
        let [mode, kind, object] = fields.as_slice() else {
            return Err("base_tree_invalid_header".to_owned());
        };
        if !matches!(*mode, "100644" | "100755") || *kind != "blob" {
            return Err(format!("base_tree_unsupported_entry:{path}"));
        }
        if !object_id(object) || !safe_path(path) {
            return Err(format!("base_tree_invalid_path_or_object:{path}"));
        }
        entries.push(Entry {
            path: path.to_owned(),
            object: (*object).to_owned(),
        });
    }
    Ok(entries)
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.chars().any(char::is_control)
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !matches!(part, "" | "." | "..") && !part.eq_ignore_ascii_case(".git"))
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn cargo_config(path: &str) -> bool {
    matches!(path, ".cargo/config" | ".cargo/config.toml")
        || path.ends_with("/.cargo/config")
        || path.ends_with("/.cargo/config.toml")
}

fn object_id(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn filename(path: &str) -> Option<&str> {
    path.rsplit('/').next()
}

fn metadata_file(path: &str) -> bool {
    matches!(filename(path), Some("Cargo.toml" | "Cargo.lock"))
}

fn read_blob(root: &Path, object: &str) -> Result<Vec<u8>, String> {
    let output = GitRequest::show(vec![OsString::from(object)])
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    if output.stdout.len() > MAX_METADATA_BYTES {
        return Err("base_tree_blob_byte_cap".to_owned());
    }
    Ok(output.stdout)
}

#[cfg(test)]
#[path = "../../test_support/git_fixture.rs"]
mod git_fixture;

#[cfg(test)]
mod tests {
    use super::{materialize, parse_entries, safe_path};
    use std::path::Path;

    fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
        let output = super::git_fixture::command(root)
            .map_err(|err| err.to_string())?
            .args(args)
            .output()
            .map_err(|err| err.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(output.stdout)
    }

    #[test]
    fn projection_preserves_manifests_and_tracked_paths() -> Result<(), String> {
        let repository = tempfile::tempdir().map_err(|err| err.to_string())?;
        let root = repository.path();
        git(root, &["init", "-q"])?;
        std::fs::create_dir_all(root.join("src")).map_err(|err| err.to_string())?;
        let manifest = b"[package]\nname = 'base-only'\nversion = '0.1.0'\n";
        std::fs::write(root.join("Cargo.toml"), manifest).map_err(|err| err.to_string())?;
        std::fs::write(root.join("Cargo.lock"), b"version = 4\n").map_err(|err| err.to_string())?;
        std::fs::write(root.join("src/lib.rs"), b"compile_error!(\"unused\");")
            .map_err(|err| err.to_string())?;
        git(root, &["add", "."])?;
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "base",
            ],
        )?;
        let sha =
            String::from_utf8(git(root, &["rev-parse", "HEAD"])?).map_err(|err| err.to_string())?;
        std::fs::remove_file(root.join("Cargo.toml")).map_err(|err| err.to_string())?;
        let (projection, manifests) = materialize(root, sha.trim())?;
        assert_eq!(manifests, ["Cargo.toml"]);
        assert_eq!(
            std::fs::read(projection.path().join("Cargo.toml")).map_err(|err| err.to_string())?,
            manifest
        );
        assert_eq!(
            std::fs::read(projection.path().join("Cargo.lock")).map_err(|err| err.to_string())?,
            b"version = 4\n"
        );
        assert!(
            std::fs::read(projection.path().join("src/lib.rs"))
                .map_err(|err| err.to_string())?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn tracked_cargo_configuration_requires_resolution() -> Result<(), String> {
        for path in [".cargo/config", "nested/.cargo/config.toml"] {
            let repository = tempfile::tempdir().map_err(|err| err.to_string())?;
            let root = repository.path();
            git(root, &["init", "-q"])?;
            let target = root.join(path);
            let parent = target.parent().ok_or_else(|| "missing parent".to_owned())?;
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            std::fs::write(target, b"[patch.crates-io]\n").map_err(|err| err.to_string())?;
            git(root, &["add", "."])?;
            git(
                root,
                &[
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "commit",
                    "-qm",
                    "base config",
                ],
            )?;
            let sha = String::from_utf8(git(root, &["rev-parse", "HEAD"])?)
                .map_err(|err| err.to_string())?;
            assert_eq!(
                materialize(root, sha.trim()).err().as_deref(),
                Some("base_graph_cargo_config_requires_resolution")
            );
        }
        Ok(())
    }

    #[test]
    fn unsupported_entries_and_unsafe_paths_are_unknown() {
        let oid = "a".repeat(40);
        for (mode, kind) in [("120000", "blob"), ("160000", "commit")] {
            let record = format!("{mode} {kind} {oid}\tCargo.toml\0");
            assert!(parse_entries(record.as_bytes()).is_err());
        }
        for path in [
            "../Cargo.toml",
            "/Cargo.toml",
            ".git/config",
            "a//b",
            "a\\b",
            "a\nb",
            "a\tb",
        ] {
            assert!(!safe_path(path));
        }
        assert!(safe_path("workspace with space/Cargo.toml"));
        assert!(parse_entries(format!("100644 blob {oid}\tCargo.toml").as_bytes()).is_err());
    }
}
