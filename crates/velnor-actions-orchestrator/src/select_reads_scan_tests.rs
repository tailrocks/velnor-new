//! Source-scan cases for out-of-directory reads.

use super::{ReadTarget, scan_source};

/// Targets one file yields as a one-file package (anchor plus escape
/// combine into `manifest_dir_escape`, as `select_reads` does per package).
fn targets(file: &str, package_dir: &str, text: &str) -> Vec<ReadTarget> {
    let scanned = scan_source(file, package_dir, text);
    let mut targets: Vec<ReadTarget> = scanned.reads.into_iter().map(|read| read.target).collect();
    if scanned.anchored && scanned.escapes {
        targets.push(ReadTarget::Unbounded("manifest_dir_escape"));
    }
    targets
}

fn path(target: &str) -> ReadTarget {
    ReadTarget::Path(target.to_owned())
}

#[test]
fn include_literal_resolves_from_the_file_directory() {
    let text = "pub const D: &str = include_str!(\"../../../libs/util/data.txt\");";
    assert_eq!(
        targets("apps/a7/src/lib.rs", "apps/a7", text),
        vec![path("libs/util/data.txt")]
    );
}

#[test]
fn include_bytes_and_include_inside_the_package_stay_local() {
    let text = "static B: &[u8] = include_bytes!(\"data.bin\"); include!(\"gen.rs\");";
    assert_eq!(
        targets("apps/a7/src/lib.rs", "apps/a7", text),
        vec![path("apps/a7/src/data.bin"), path("apps/a7/src/gen.rs")]
    );
}

#[test]
fn manifest_dir_concat_resolves_from_the_package_directory() {
    let text = "#[test] fn t(){ std::fs::read(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../libs/util/fixtures/x.json\")).unwrap(); }";
    assert_eq!(
        targets("apps/a7/tests/t.rs", "apps/a7", text),
        vec![path("libs/util/fixtures/x.json")]
    );
}

#[test]
fn out_dir_includes_read_build_output() {
    let text = "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));";
    assert_eq!(targets("apps/a7/src/lib.rs", "apps/a7", text), Vec::new());
}

#[test]
fn computed_include_arguments_are_unbounded() {
    let text = "include_str!(some_macro!());";
    assert_eq!(
        targets("apps/a7/src/lib.rs", "apps/a7", text),
        vec![ReadTarget::Unbounded("include_argument")]
    );
}

#[test]
fn path_attributes_resolve_from_the_file_directory() {
    let text = "#[path = \"../../shared/m.rs\"] mod m; fn f() { let path = \"../x\"; }";
    assert_eq!(
        targets("crates/a/src/lib.rs", "crates/a", text),
        vec![path("crates/shared/m.rs")]
    );
}

#[test]
fn manifest_dir_with_parent_components_is_unbounded() {
    let joined = "let root = Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../..\");";
    let parent = "let root = Path::new(env!(\"CARGO_MANIFEST_DIR\")).parent().unwrap();";
    for text in [joined, parent] {
        assert_eq!(
            targets("crates/cli/tests/t.rs", "crates/cli", text),
            vec![ReadTarget::Unbounded("manifest_dir_escape")],
            "{text}"
        );
    }
}

#[test]
fn resolved_manifest_dir_concat_still_anchors_parent_walks() {
    let text = "let p = Path::new(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/Cargo.toml\")).parent().unwrap().parent();";
    assert_eq!(
        targets("crates/a/tests/t.rs", "crates/a", text),
        vec![
            path("crates/a/Cargo.toml"),
            ReadTarget::Unbounded("manifest_dir_escape")
        ]
    );
}

#[test]
fn manifest_dir_without_escape_is_local() {
    let text = "let f = Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"tests/data\");";
    assert_eq!(
        targets("crates/cli/tests/t.rs", "crates/cli", text),
        Vec::new()
    );
}

#[test]
fn relative_fs_calls_resolve_from_the_package_directory() {
    let text = "let s = std::fs::read_to_string(\"../other/Cargo.toml\");";
    assert_eq!(
        targets("crates/cli/tests/t.rs", "crates/cli", text),
        vec![path("crates/other/Cargo.toml")]
    );
}

#[test]
fn traversal_literals_without_reads_are_not_reads() {
    let text = "assert!(validate(\"../etc/passwd\").is_err()); let p = Path::new(\"../x\");";
    assert_eq!(
        targets("crates/cli/src/v.rs", "crates/cli", text),
        Vec::new()
    );
}

#[test]
fn comments_never_count() {
    let text = "// include_str!(\"../../x\")\n/* env!(\"CARGO_MANIFEST_DIR\") \"../\" */";
    assert_eq!(
        targets("crates/cli/src/v.rs", "crates/cli", text),
        Vec::new()
    );
}

#[test]
fn absolute_and_escaping_includes_are_outside() {
    let text = "include_str!(\"/etc/hosts\"); include_str!(\"../../../../x\");";
    assert_eq!(
        targets("crates/a/src/lib.rs", "crates/a", text),
        vec![ReadTarget::Outside, ReadTarget::Outside]
    );
}
