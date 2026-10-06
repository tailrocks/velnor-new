//! Exact expansion evidence, isolated from filesystem admin/output state.

use std::path::Path;

use velnor_actions_rust::WorkspaceRecord;

use super::{NodeKind, capture};

fn records(root: &Path, fields: &str) -> Vec<(String, WorkspaceRecord)> {
    std::fs::write(root.join("Cargo.toml"), format!("[workspace]\n{fields}\n")).expect("workspace");
    vec![(
        "Cargo.toml".to_owned(),
        WorkspaceRecord {
            workspace_root: String::new(),
            members: Vec::new(),
            packages: Vec::new(),
            edges: Vec::new(),
            skipped_edges: Vec::new(),
        },
    )]
}

fn member(root: &Path, relative: &str) {
    std::fs::create_dir_all(root.join(relative)).expect("member directory");
    std::fs::write(
        root.join(relative).join("Cargo.toml"),
        "[package]\nname='example'\nversion='1.0.0'\n",
    )
    .expect("member manifest");
}

fn digest(root: &Path, records: &[(String, WorkspaceRecord)]) -> String {
    crate::analysis_inventory::resolution_inputs_digest(root, &["Cargo.toml".to_owned()], records)
        .expect("input identity")
}

#[test]
fn unrelated_admin_and_output_layout_preserves_identity() {
    let root = tempfile::tempdir().expect("root");
    member(root.path(), "crates/base");
    let records = records(root.path(), "members=['crates/*']");
    let before = digest(root.path(), &records);
    for directory in [
        ".git/objects/pack",
        ".git/refs/heads",
        "target/debug/deps",
        ".velnor/analysis-tmp",
    ] {
        std::fs::create_dir_all(root.path().join(directory)).expect("unrelated directory");
        std::fs::write(
            root.path().join(directory).join("Cargo.toml"),
            "output only",
        )
        .expect("unrelated manifest");
    }
    assert_eq!(before, digest(root.path(), &records));
}

#[test]
fn hidden_member_and_manifest_changes_invalidate() {
    let root = tempfile::tempdir().expect("root");
    member(root.path(), "crates/base");
    let records = records(root.path(), "members=['crates/*']");
    let before = digest(root.path(), &records);
    member(root.path(), "crates/.hidden");
    let hidden = digest(root.path(), &records);
    assert_ne!(before, hidden);
    std::fs::write(
        root.path().join("crates/.hidden/Cargo.toml"),
        "[package]\nname='different'\nversion='2.0.0'\n",
    )
    .expect("changed hidden manifest");
    assert_ne!(hidden, digest(root.path(), &records));
}

#[test]
fn excluded_empty_matching_directory_remains_bound() {
    let root = tempfile::tempdir().expect("root");
    member(root.path(), "crates/base");
    let records = records(
        root.path(),
        "members=['crates/*']\nexclude=['crates/ignored']",
    );
    let before = digest(root.path(), &records);
    std::fs::create_dir(root.path().join("crates/ignored")).expect("excluded directory");
    assert_ne!(before, digest(root.path(), &records));
    let expansions = capture(root.path(), &records).expect("expansion");
    assert!(
        expansions[0]
            .matches
            .iter()
            .any(|matched| matched.path == "crates/ignored" && matched.manifest.is_none())
    );
    let empty = digest(root.path(), &records);
    member(root.path(), "crates/ignored");
    let manifest = digest(root.path(), &records);
    assert_ne!(empty, manifest);
    std::fs::write(
        root.path().join("crates/ignored/Cargo.toml"),
        "[package]\nname='changed'\nversion='2.0.0'\n",
    )
    .expect("excluded manifest mutation");
    assert_ne!(manifest, digest(root.path(), &records));
}

#[test]
fn files_only_matches_do_not_take_zero_match_fallback() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("crates")).expect("parent");
    let records = records(root.path(), "members=['crates/file*']");
    let zero = capture(root.path(), &records).expect("zero matches");
    assert!(zero[0].literal_fallback);
    assert!(matches!(zero[0].matches[0].kind, NodeKind::Absent));
    std::fs::write(root.path().join("crates/file_only"), "file").expect("matching file");
    let files = capture(root.path(), &records).expect("file match");
    assert!(!files[0].literal_fallback);
    assert!(matches!(files[0].matches[0].kind, NodeKind::File));
    assert_eq!(files[0].matches[0].path, "crates/file_only");
    let file = digest(root.path(), &records);
    std::fs::remove_file(root.path().join("crates/file_only")).expect("remove match file");
    std::fs::create_dir(root.path().join("crates/file_only")).expect("replace match directory");
    let directory = capture(root.path(), &records).expect("directory match");
    assert!(!directory[0].literal_fallback);
    assert!(matches!(directory[0].matches[0].kind, NodeKind::Directory));
    assert_ne!(file, digest(root.path(), &records));
}

#[test]
fn literal_presence_and_node_kind_are_distinct() {
    let root = tempfile::tempdir().expect("root");
    let records = records(root.path(), "members=['member']");
    let absent = digest(root.path(), &records);
    std::fs::write(root.path().join("member"), "file").expect("literal file");
    let file = digest(root.path(), &records);
    assert_ne!(absent, file);
    std::fs::remove_file(root.path().join("member")).expect("remove file");
    std::fs::create_dir(root.path().join("member")).expect("literal directory");
    assert_ne!(file, digest(root.path(), &records));
}

#[test]
fn default_members_have_their_own_complete_expansion() {
    let root = tempfile::tempdir().expect("root");
    member(root.path(), "crates/base");
    let records = records(
        root.path(),
        "members=['crates/base']\ndefault-members=['crates/base*']",
    );
    let expansions = capture(root.path(), &records).expect("both fields");
    assert_eq!(expansions.len(), 2);
    assert_eq!(expansions[0].field, "members");
    assert_eq!(expansions[1].field, "default-members");
    assert_eq!(expansions[1].matches[0].path, "crates/base");
    let before = digest(root.path(), &records);
    member(root.path(), "crates/base_next");
    let changed = capture(root.path(), &records).expect("default-only match");
    assert_eq!(changed[0].matches.len(), 1);
    assert_eq!(changed[1].matches.len(), 2);
    assert_ne!(before, digest(root.path(), &records));
}

#[test]
fn expansion_parent_absence_file_and_directory_are_bound() {
    let root = tempfile::tempdir().expect("root");
    let records = records(root.path(), "members=['crates/member*']");
    let absent = capture(root.path(), &records).expect("absent parent");
    assert!(matches!(absent[0].parent.kind, NodeKind::Absent));
    let absent_digest = digest(root.path(), &records);
    std::fs::write(root.path().join("crates"), "parent file").expect("file parent");
    let file = capture(root.path(), &records).expect("file parent");
    assert!(matches!(file[0].parent.kind, NodeKind::File));
    let file_digest = digest(root.path(), &records);
    assert_ne!(absent_digest, file_digest);
    std::fs::remove_file(root.path().join("crates")).expect("remove parent file");
    std::fs::create_dir(root.path().join("crates")).expect("empty parent directory");
    let directory = capture(root.path(), &records).expect("directory parent");
    assert!(matches!(directory[0].parent.kind, NodeKind::Directory));
    assert_ne!(file_digest, digest(root.path(), &records));
}

#[test]
fn nested_target_member_is_supported_and_opaque_scopes_fall_back() {
    let root = tempfile::tempdir().expect("root");
    member(root.path(), "crates/target");
    let supported = records(root.path(), "members=['crates/target']");
    assert!(capture(root.path(), &supported).is_ok());
    for pattern in [
        "*",
        "target/*",
        ".git/*",
        ".velnor/*",
        "crates/**",
        "*/member",
        "crates/?",
        "crates/[ab]",
        "../escape",
    ] {
        let unsupported = records(root.path(), &format!("members=['{pattern}']"));
        assert!(
            capture(root.path(), &unsupported).is_err(),
            "accepted {pattern}"
        );
    }
}

#[test]
#[cfg(unix)]
fn symlinked_glob_parent_and_matching_member_are_refused() {
    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("outside");
    member(outside.path(), "member");
    let records = records(root.path(), "members=['crates/*']");
    std::os::unix::fs::symlink(outside.path(), root.path().join("crates")).expect("parent symlink");
    assert!(capture(root.path(), &records).is_err());
    std::fs::remove_file(root.path().join("crates")).expect("remove link");
    std::fs::create_dir(root.path().join("crates")).expect("real parent");
    std::os::unix::fs::symlink(
        outside.path().join("member"),
        root.path().join("crates/member"),
    )
    .expect("member symlink");
    assert!(capture(root.path(), &records).is_err());
}

#[test]
#[cfg(unix)]
fn non_utf8_expansion_entries_refuse_reuse() {
    use std::os::unix::ffi::OsStringExt;
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("crates")).expect("parent");
    let records = records(root.path(), "members=['crates/*']");
    std::fs::create_dir(
        root.path()
            .join("crates")
            .join(std::ffi::OsString::from_vec(vec![0xff])),
    )
    .expect("opaque directory");
    assert!(capture(root.path(), &records).is_err());
}
