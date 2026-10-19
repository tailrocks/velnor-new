//! Skeleton planning: members stubbed, `[patch]` path closure kept real.

use std::path::{Path, PathBuf};

use super::{BIN_STUB, LIB_STUB, Metadata, SHARED_STUB, plan_stubs};

fn write(root: &Path, rel: &str) -> PathBuf {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(&path, "pub fn real() {}\n").expect("source");
    path
}

fn target(kind: &str, path: &Path) -> serde_json::Value {
    serde_json::json!({"kind": [kind], "crate_types": [kind], "name": "t", "src_path": path})
}

/// `app` (member) uses `patched` (member, `[patch]` target of registry
/// `reg`), which uses `inner` (non-member path crate); `tool` is a member
/// whose lib and bin share one file.
fn metadata(root: &Path) -> Metadata {
    let app_lib = write(root, "app/src/lib.rs");
    let app_bin = write(root, "app/src/main.rs");
    let app_test = write(root, "app/tests/it.rs");
    let app_build = write(root, "app/build.rs");
    let patched = write(root, "vendor/patched/src/lib.rs");
    let inner = write(root, "vendor/inner/src/lib.rs");
    let shared = write(root, "tool/src/lib.rs");
    let value = serde_json::json!({
        "packages": [
            {"id": "path+file:///ws/app#0.1.0", "source": null, "targets": [
                target("lib", &app_lib), target("bin", &app_bin),
                target("test", &app_test), target("custom-build", &app_build)]},
            {"id": "path+file:///ws/vendor/patched#1.0.0", "source": null, "targets": [target("lib", &patched)]},
            {"id": "path+file:///ws/vendor/inner#1.0.0", "source": null, "targets": [target("lib", &inner)]},
            {"id": "path+file:///ws/tool#0.1.0", "source": null, "targets": [
                target("lib", &shared), target("bin", &shared)]},
            {"id": "registry+https://github.com/rust-lang/crates.io-index#reg@2.0.0",
             "source": "registry+https://github.com/rust-lang/crates.io-index", "targets": []}
        ],
        "workspace_members": [
            "path+file:///ws/app#0.1.0", "path+file:///ws/vendor/patched#1.0.0", "path+file:///ws/tool#0.1.0"
        ],
        "resolve": {"nodes": [
            {"id": "path+file:///ws/app#0.1.0", "deps": [
                {"pkg": "registry+https://github.com/rust-lang/crates.io-index#reg@2.0.0"}]},
            {"id": "registry+https://github.com/rust-lang/crates.io-index#reg@2.0.0", "deps": [
                {"pkg": "path+file:///ws/vendor/patched#1.0.0"}]},
            {"id": "path+file:///ws/vendor/patched#1.0.0", "deps": [
                {"pkg": "path+file:///ws/vendor/inner#1.0.0"}]},
            {"id": "path+file:///ws/vendor/inner#1.0.0", "deps": []},
            {"id": "path+file:///ws/tool#0.1.0", "deps": []}
        ]}
    });
    serde_json::from_value(value).expect("metadata")
}

fn workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    (dir, root)
}

#[test]
fn members_are_stubbed_and_the_patch_closure_keeps_its_sources() {
    let (_dir, root) = workspace();
    let metadata = metadata(&root);
    let protected = metadata.protected_path_packages();
    assert!(protected.contains("path+file:///ws/vendor/patched#1.0.0"));
    assert!(protected.contains("path+file:///ws/vendor/inner#1.0.0"));
    assert!(!protected.contains("path+file:///ws/app#0.1.0"));
    let stubs = plan_stubs(&metadata, &root).expect("plan");
    let stub = |rel: &str| stubs.get(&root.join(rel)).copied();
    assert_eq!(stub("app/src/lib.rs"), Some(LIB_STUB));
    for rel in ["app/src/main.rs", "app/tests/it.rs", "app/build.rs"] {
        assert_eq!(stub(rel), Some(BIN_STUB), "{rel}");
    }
    assert_eq!(stub("tool/src/lib.rs"), Some(SHARED_STUB));
    assert_eq!(stub("vendor/patched/src/lib.rs"), None);
    assert_eq!(stub("vendor/inner/src/lib.rs"), None);
    assert_eq!(stubs.len(), 5);
}

#[test]
fn sources_outside_the_workspace_or_symlinked_are_refused() {
    let (_dir, root) = workspace();
    let (_outside_dir, outside) = workspace();
    let foreign = write(&outside, "src/lib.rs");
    let value = serde_json::json!({
        "packages": [{"id": "path+file:///x#0.1.0", "source": null, "targets": [target("lib", &foreign)]}],
        "workspace_members": ["path+file:///x#0.1.0"],
        "resolve": {"nodes": []}
    });
    let metadata: Metadata = serde_json::from_value(value).expect("metadata");
    let err = plan_stubs(&metadata, &root).expect_err("outside");
    assert!(
        err.to_string().contains("seed_source_outside_workspace"),
        "{err}"
    );
    #[cfg(unix)]
    {
        let link = root.join("link.rs");
        std::os::unix::fs::symlink(&foreign, &link).expect("symlink");
        let value = serde_json::json!({
            "packages": [{"id": "path+file:///x#0.1.0", "source": null, "targets": [target("lib", &link)]}],
            "workspace_members": ["path+file:///x#0.1.0"],
            "resolve": {"nodes": []}
        });
        let metadata: Metadata = serde_json::from_value(value).expect("metadata");
        let err = plan_stubs(&metadata, &root).expect_err("symlink");
        assert!(err.to_string().contains("seed_source_not_regular"), "{err}");
    }
}
