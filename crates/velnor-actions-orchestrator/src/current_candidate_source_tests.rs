use super::*;
use serde_json::json;

const COMMIT: &str = "1111111111111111111111111111111111111111";
const TREE: &str = "2222222222222222222222222222222222222222";
const BLOB: &str = "3333333333333333333333333333333333333333";

fn tree(entries: serde_json::Value) -> Vec<u8> {
    json!({"sha": TREE, "truncated": false, "tree": entries})
        .to_string()
        .into_bytes()
}

fn blob(path: &str, mode: &str, size: u64) -> serde_json::Value {
    json!({"path": path, "mode": mode, "type": "blob", "sha": BLOB, "size": size})
}

fn directory(path: &str) -> serde_json::Value {
    json!({"path": path, "mode": "040000", "type": "tree", "sha": TREE})
}

#[test]
fn complete_protocol_binds_binary_bytes_and_exact_endpoints()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::TempDir::new()?;
    fs::create_dir(root.path().join("nested"))?;
    let bytes = vec![0, 255, 254, 13, 10];
    fs::write(root.path().join("nested/input"), &bytes)?;
    let mut calls = Vec::new();
    verify_with(root.path(), "owner/repo", COMMIT, |endpoint, raw| {
        calls.push((endpoint.to_owned(), raw));
        if endpoint.contains("/commits/") {
            return Ok(json!({"sha": COMMIT, "tree": {"sha": TREE}})
                .to_string()
                .into_bytes());
        }
        if endpoint.contains("/trees/") {
            return Ok(tree(json!([
                directory("nested"),
                blob("nested/input", "100644", 5)
            ])));
        }
        Ok(bytes.clone())
    })?;
    assert_eq!(
        calls,
        vec![
            (format!("repos/owner/repo/git/commits/{COMMIT}"), false),
            (
                format!("repos/owner/repo/git/trees/{TREE}?recursive=1"),
                false
            ),
            (format!("repos/owner/repo/git/blobs/{BLOB}"), true),
        ]
    );
    Ok(())
}

#[test]
fn commit_response_requires_exact_identity_and_strict_json() {
    let good = json!({"sha": COMMIT, "tree": {"sha": TREE}}).to_string();
    assert_eq!(
        commit_tree(good.as_bytes(), COMMIT).ok().as_deref(),
        Some(TREE)
    );
    assert!(commit_tree(good.as_bytes(), TREE).is_err());
    for sha in [
        "short",
        "0000000000000000000000000000000000000000",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        let text = json!({"sha": COMMIT, "tree": {"sha": sha}}).to_string();
        assert!(commit_tree(text.as_bytes(), COMMIT).is_err());
    }
    let duplicate = format!(r#"{{"sha":"{COMMIT}","sha":"{COMMIT}","tree":{{"sha":"{TREE}"}}}}"#);
    assert!(commit_tree(duplicate.as_bytes(), COMMIT).is_err());
    assert!(commit_tree(&[255], COMMIT).is_err());
}

#[test]
fn tree_rejects_truncation_missing_identity_and_duplicate_keys() {
    for value in [
        json!({"sha": TREE, "truncated": true, "tree": []}),
        json!({"sha": TREE, "tree": []}),
        json!({"sha": COMMIT, "truncated": false, "tree": []}),
        json!({"sha": TREE, "truncated": false, "tree": null}),
    ] {
        assert!(parse_tree(value.to_string().as_bytes(), TREE).is_err());
    }
    let text = format!(r#"{{"sha":"{TREE}","truncated":false,"truncated":false,"tree":[]}}"#);
    assert!(parse_tree(text.as_bytes(), TREE).is_err());
}

#[test]
fn tree_paths_kinds_modes_sizes_and_topology_fail_closed() {
    for path in [
        "",
        "/input",
        "../input",
        "./input",
        "nested//input",
        "nested\\input",
        "input\n",
        ".git/config",
        ".GIT/config",
    ] {
        assert!(
            parse_tree(&tree(json!([blob(path, "100644", 0)])), TREE).is_err(),
            "{path}"
        );
    }
    for (kind, mode) in [
        ("blob", "120000"),
        ("commit", "160000"),
        ("tree", "100644"),
        ("blob", "040000"),
        ("blob", "100600"),
    ] {
        let mut entry = blob("input", mode, 0);
        entry["type"] = kind.into();
        assert!(parse_tree(&tree(json!([entry])), TREE).is_err());
    }
    let entry = blob("input", "100644", 0);
    assert!(parse_tree(&tree(json!([entry.clone(), entry])), TREE).is_err());
    assert!(parse_tree(&tree(json!([blob("nested/input", "100644", 0)])), TREE).is_err());
    assert!(parse_tree(&tree(json!([directory("empty")])), TREE).is_err());
    let mut missing_size = blob("input", "100644", 0);
    assert!(
        missing_size
            .as_object_mut()
            .expect("object")
            .remove("size")
            .is_some()
    );
    assert!(parse_tree(&tree(json!([missing_size])), TREE).is_err());
}

#[test]
fn extra_missing_or_substituted_checkout_bytes_never_grant()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::TempDir::new()?;
    let blobs = parse_tree(&tree(json!([blob("input", "100644", 3)])), TREE)?;
    assert!(compare_source(root.path(), &blobs, |_| Ok(b"old".to_vec())).is_err());
    fs::write(root.path().join("input"), b"old")?;
    compare_source(root.path(), &blobs, |_| Ok(b"old".to_vec()))?;
    assert!(compare_source(root.path(), &blobs, |_| Ok(b"new".to_vec())).is_err());
    assert!(compare_source(root.path(), &blobs, |_| Ok(b"old!".to_vec())).is_err());
    fs::write(root.path().join("untracked"), b"extra")?;
    let mut read_called = false;
    assert!(
        compare_source(root.path(), &blobs, |_| {
            read_called = true;
            Ok(b"old".to_vec())
        })
        .is_err()
    );
    assert!(!read_called);
    Ok(())
}

#[test]
#[cfg(unix)]
fn executable_modes_and_symlink_substitution_fail_closed() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let root = tempfile::TempDir::new()?;
    let path = root.path().join("script");
    fs::write(&path, b"run")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
    let blobs = parse_tree(&tree(json!([blob("script", "100755", 3)])), TREE)?;
    assert!(compare_source(root.path(), &blobs, |_| Ok(b"run".to_vec())).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    compare_source(root.path(), &blobs, |_| Ok(b"run".to_vec()))?;
    let external = tempfile::NamedTempFile::new()?;
    fs::remove_file(&path)?;
    symlink(external.path(), &path)?;
    assert!(compare_source(root.path(), &blobs, |_| Ok(b"run".to_vec())).is_err());
    Ok(())
}

#[test]
#[cfg(unix)]
fn git_owner_executable_bit_controls_service_mode() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    let root = tempfile::TempDir::new()?;
    let path = root.path().join("script");
    fs::write(&path, b"run")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o654))?;
    let executable = parse_tree(&tree(json!([blob("script", "100755", 3)])), TREE)?;
    assert!(compare_source(root.path(), &executable, |_| Ok(b"run".to_vec())).is_err());
    let regular = parse_tree(&tree(json!([blob("script", "100644", 3)])), TREE)?;
    compare_source(root.path(), &regular, |_| Ok(b"run".to_vec()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o744))?;
    assert!(compare_source(root.path(), &regular, |_| Ok(b"run".to_vec())).is_err());
    compare_source(root.path(), &executable, |_| Ok(b"run".to_vec()))?;
    Ok(())
}

#[test]
fn declared_blob_and_aggregate_sizes_are_bounded_before_checkout_reads() {
    assert!(
        parse_tree(
            &tree(json!([blob("input", "100644", MAX_BLOB_BYTES + 1)])),
            TREE
        )
        .is_err()
    );
    let entries: Vec<_> = (0..=MAX_SOURCE_BYTES / MAX_BLOB_BYTES)
        .map(|index| blob(&format!("input{index}"), "100644", MAX_BLOB_BYTES))
        .collect();
    assert!(parse_tree(&tree(json!(entries)), TREE).is_err());
    assert!(
        parse_tree(
            &tree(json!([blob("input", "100644", MAX_BLOB_BYTES)])),
            TREE
        )
        .is_ok()
    );
}

#[test]
fn pinned_reader_checks_declared_size_and_bounds_growth() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::TempDir::new()?;
    fs::write(root.path().join("input"), b"old")?;
    let fd = open(root.path(), directory_flags(), Mode::empty())?;
    assert!(read_file(&fd, "input", 2).is_err());
    assert!(read_file(&fd, "input", 4).is_err());
    assert_eq!(read_file(&fd, "input", 3)?.0, b"old");
    assert!(bounded_bytes(std::io::Cursor::new(b"grew"), 3).is_err());
    assert!(bounded_bytes(std::io::Cursor::new(b"short"), 6).is_err());
    assert!(bounded_bytes(std::io::Cursor::new(b""), MAX_BLOB_BYTES + 1).is_err());
    Ok(())
}

#[test]
fn public_entry_denies_until_authenticated_transport_exists()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::TempDir::new()?;
    let error = verify(root.path(), root.path(), "owner/repo", COMMIT)
        .expect_err("uncleared transport cannot authenticate current candidate");
    assert!(
        error
            .to_string()
            .contains("candidate_source_authenticated_transport_pending")
    );
    Ok(())
}
