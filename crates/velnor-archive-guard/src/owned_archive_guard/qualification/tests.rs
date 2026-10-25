use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use super::{validate_qualification_manifest, validate_qualification_output_bindings};

#[path = "limit_tests.rs"]
mod limit_tests;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create scratch");
        let path = dir.path().canonicalize().expect("physical scratch path");
        let _kept_directory = dir.keep();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn manifest() -> String {
    r#"{"schema":1,"version":"0.1.52","repository":"tailrocks/velnor-new","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","targets":[{"target":"x86_64-unknown-linux-gnu","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-unknown-linux-gnu","sha256":"1111111111111111111111111111111111111111111111111111111111111111"},{"target":"aarch64-apple-darwin","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-aarch64-apple-darwin","sha256":"2222222222222222222222222222222222222222222222222222222222222222"},{"target":"x86_64-apple-darwin","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-apple-darwin","sha256":"3333333333333333333333333333333333333333333333333333333333333333"}]}"#.to_owned()
}

fn valid_workflow() -> &'static str {
    "      VELNOR_ASSET_SHA256: 1111111111111111111111111111111111111111111111111111111111111111\n      VELNOR_ASSET_URL: https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-unknown-linux-gnu\n      VELNOR_RELEASE_COMMIT: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"
}

fn setup() -> (Scratch, PathBuf, PathBuf) {
    let scratch = Scratch::new();
    let manifest_path = scratch.path("manifest.json");
    fs::write(&manifest_path, manifest()).expect("write manifest");
    let workflows = scratch.path(".github");
    fs::create_dir(&workflows).expect("create workflow root");
    (scratch, manifest_path, workflows)
}

#[test]
fn exact_manifest_and_adjacent_binding_are_accepted() {
    let (_scratch, manifest_path, workflows) = setup();
    fs::write(workflows.join("ci.yml"), valid_workflow()).expect("write workflow");
    assert!(validate_qualification_manifest(&manifest_path).is_ok());
    assert!(validate_qualification_output_bindings(&manifest_path, &workflows).is_ok());
}

#[test]
fn rejects_duplicate_keys_and_manifest_identity_inventory_faults() {
    let (_scratch, path, _workflows) = setup();
    for (contents, expected) in [
        (r#"{"schema":1,"schema":1}"#, "duplicate object key"),
        (r#"{"schema":1e9999}"#, "invalid JSON"),
        (r#"{"schema":1} trailing"#, "trailing JSON"),
        (
            r#"{"schema":1,"repository":"wrong"}"#,
            "schema or repository",
        ),
        (
            r#"{"schema":1,"repository":"tailrocks/velnor-new","version":"0.01.2"}"#,
            "version is malformed",
        ),
    ] {
        fs::write(&path, contents).expect("write manifest fault");
        let error = validate_qualification_manifest(&path).expect_err("manifest rejected");
        assert!(error.contains(expected), "{error}");
    }
    fs::write(&path, [0xff]).expect("write invalid UTF-8 JSON");
    assert!(validate_qualification_manifest(&path).is_err());
}

#[test]
fn rejects_binding_digest_commit_url_scope_and_missing_field_faults() {
    let (_scratch, manifest_path, workflows) = setup();
    let original = valid_workflow();
    let wrong_digest = original.replace(
        "1111111111111111111111111111111111111111111111111111111111111111",
        "4444444444444444444444444444444444444444444444444444444444444444",
    );
    let wrong_commit = original.replace(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );
    let wrong_url = original.replace("https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-unknown-linux-gnu", "https://example.invalid/unknown");
    let wrong_scope = original.replace("      VELNOR_ASSET_URL", "    VELNOR_ASSET_URL");
    let missing = original
        .lines()
        .filter(|line| !line.contains("VELNOR_RELEASE_COMMIT"))
        .collect::<Vec<_>>()
        .join("\n");
    for (contents, expected) in [
        (wrong_digest, "wrong digest for target"),
        (wrong_commit, "wrong source commit"),
        (wrong_url, "unknown release URL"),
        (wrong_scope, "different scopes"),
        (missing, "incomplete binding"),
    ] {
        fs::write(workflows.join("ci.yml"), contents).expect("write workflow fault");
        let error = validate_qualification_output_bindings(&manifest_path, &workflows)
            .expect_err("binding rejected");
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn skips_leaf_symlinks_and_rejects_root_symlinks() {
    let (_scratch, manifest_path, workflows) = setup();
    #[cfg(unix)]
    {
        symlink("missing", workflows.join("link.yml")).expect("create symlink");
        fs::write(workflows.join("ci.yml"), valid_workflow()).expect("write valid workflow");
        assert!(validate_qualification_output_bindings(&manifest_path, &workflows).is_ok());
        fs::remove_dir_all(&workflows).expect("remove workflow root");
        symlink("missing", &workflows).expect("link root");
        assert!(validate_qualification_output_bindings(&manifest_path, &workflows).is_err());
    }
}

#[test]
#[cfg(unix)]
fn manifest_symlink_is_not_followed() {
    let (scratch, manifest_path, _) = setup();
    let linked = scratch.path("linked-manifest.json");
    symlink(&manifest_path, &linked).expect("link manifest");
    assert!(validate_qualification_manifest(&linked).is_err());
}
