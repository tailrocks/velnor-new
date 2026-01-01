//! Read-only task-cache helper cases.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use velnor_actions_mise_cache::{
    CachedTaskDescriptor, TaskCacheMode, artifact_path, read_artifact_bytes,
    resolve_task_artifact_dir, verify_artifact_digest,
};
use velnor_actions_mise_core::MiseError;

fn scratch_dir(test: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("velnor-mise-{test}-{}", std::process::id()));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) | Err(_) => {}
    }
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

fn sample_descriptor() -> CachedTaskDescriptor {
    CachedTaskDescriptor {
        task_name: "clippy-demo".to_owned(),
        sources: vec!["src/**/*.rs".to_owned()],
        outputs: vec!["report.json".to_owned()],
        command_inputs: vec!["cargo clippy --version".to_owned()],
        env: BTreeMap::from([("RUSTFLAGS".to_owned(), "-D warnings".to_owned())]),
        tools: vec!["rust@1.98.1".to_owned()],
        dep_keys: Vec::new(),
    }
}

#[test]
fn cache_modes_roundtrip_display_parse() {
    for mode in [
        TaskCacheMode::ReadWrite,
        TaskCacheMode::ReadOnly,
        TaskCacheMode::WriteOnly,
        TaskCacheMode::Off,
        TaskCacheMode::LocalOnly,
    ] {
        let text = mode.to_string();
        assert_eq!(text.parse::<TaskCacheMode>(), Ok(mode));
    }
    assert_eq!(TaskCacheMode::ReadOnly.to_string(), "read-only");
    for raw in ["readwrite", "READ-ONLY", "off ", ""] {
        assert!(
            matches!(
                raw.parse::<TaskCacheMode>(),
                Err(MiseError::UnknownCacheMode { .. })
            ),
            "mode must be rejected: {raw}"
        );
    }
}

#[test]
fn artifact_dir_prefers_task_cache_parent() {
    let task_parent = Path::new("/tmp/task-cache");
    let default_parent = Path::new("/tmp/mise-cache");
    assert_eq!(
        resolve_task_artifact_dir(Some(task_parent), Some(default_parent)),
        Some(PathBuf::from("/tmp/task-cache/task-artifacts/v2"))
    );
    assert_eq!(
        resolve_task_artifact_dir(None, Some(default_parent)),
        Some(PathBuf::from("/tmp/mise-cache/task-artifacts/v2"))
    );
    assert_eq!(resolve_task_artifact_dir(None, None), None);
}

#[test]
fn artifact_path_joins_and_rejects_escapes() {
    let root = Path::new("/tmp/task-cache/task-artifacts/v2");
    assert_eq!(
        artifact_path(root, "clippy/report.json"),
        Ok(PathBuf::from(
            "/tmp/task-cache/task-artifacts/v2/clippy/report.json"
        ))
    );
    assert_eq!(
        artifact_path(root, "a/./b"),
        Ok(PathBuf::from("/tmp/task-cache/task-artifacts/v2/a/b"))
    );
    for relative in ["/etc/passwd", "..", "../escape", "a/../b", ".", ""] {
        assert!(
            matches!(
                artifact_path(root, relative),
                Err(MiseError::ArtifactEscapesRoot { .. })
            ),
            "path must be rejected: {relative}"
        );
    }
}

#[test]
fn read_and_verify_artifact_roundtrip() -> Result<(), String> {
    let dir = scratch_dir("cache-artifact")?;
    let path = dir.join("report.json");
    std::fs::write(&path, b"{\"ok\":true}").map_err(|err| err.to_string())?;
    let bytes = read_artifact_bytes(&path).map_err(|err| err.to_string())?;
    assert_eq!(bytes, b"{\"ok\":true}");

    let expected = velnor_actions_contract::digest_b3(&bytes);
    verify_artifact_digest(&bytes, &expected).map_err(|err| err.to_string())?;
    assert!(matches!(
        verify_artifact_digest(&bytes, &velnor_actions_contract::digest_b3(b"other")),
        Err(MiseError::DigestMismatch { .. })
    ));
    assert!(matches!(
        verify_artifact_digest(&bytes, "not-a-digest"),
        Err(MiseError::InvalidDigest { .. })
    ));
    assert!(matches!(
        read_artifact_bytes(&dir.join("missing.json")),
        Err(MiseError::ArtifactNotFound { .. })
    ));
    Ok(())
}

#[test]
fn descriptor_digest_is_deterministic() -> Result<(), String> {
    let descriptor = sample_descriptor();
    descriptor.validate().map_err(|err| err.to_string())?;
    let first = descriptor
        .cache_inputs_digest()
        .map_err(|err| err.to_string())?;
    let second = sample_descriptor()
        .cache_inputs_digest()
        .map_err(|err| err.to_string())?;
    assert_eq!(first, second);
    assert!(velnor_actions_contract::is_valid_digest(&first));

    let mut changed = sample_descriptor();
    changed.env.insert("EXTRA".to_owned(), "1".to_owned());
    let other = changed
        .cache_inputs_digest()
        .map_err(|err| err.to_string())?;
    assert_ne!(first, other);
    Ok(())
}

#[test]
fn descriptor_without_sources_is_not_eligible() {
    let mut descriptor = sample_descriptor();
    descriptor.sources.clear();
    assert!(matches!(
        descriptor.validate(),
        Err(MiseError::CacheNotEligible { .. })
    ));
    assert!(matches!(
        descriptor.cache_inputs_digest(),
        Err(MiseError::CacheNotEligible { .. })
    ));
}
