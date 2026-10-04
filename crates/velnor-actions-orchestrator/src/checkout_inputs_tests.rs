#[path = "../../test_support/git_fixture.rs"]
mod git_fixture;

use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::Provenance;

use super::{parse_inventory, resolve};

#[test]
fn oversized_checkout_input_is_unknown_before_capture() {
    let root = TempDir::new().expect("temp checkout");
    let file = std::fs::File::create(root.path().join("large.bin")).expect("create sparse input");
    let cap = velnor_actions_mise::command::OUTPUT_CAPTURE_LIMIT_BYTES as u64;
    file.set_len(cap + 1).expect("size sparse input");
    assert!(matches!(
        resolve(root.path()),
        Provenance::Unknown { reason } if reason.contains("checkout_input_capture_limit")
    ));
}

fn git(root: &Path, args: &[&str]) {
    let output = git_fixture::command(root)
        .expect("isolate fixture git")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(root: &Path, path: &str, contents: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().expect("parent")).expect("create parent");
    std::fs::write(full, contents).expect("write input");
}

fn checkout() -> TempDir {
    let temp = TempDir::new().expect("temp checkout");
    git(temp.path(), &["init", "--quiet"]);
    write(temp.path(), "Cargo.toml", "[workspace]\nmembers = []\n");
    git(temp.path(), &["add", "."]);
    temp
}

fn digest(root: &Path) -> String {
    let provenance = resolve(root);
    assert!(
        matches!(&provenance, Provenance::Known { .. }),
        "expected complete identity: {provenance:?}"
    );
    match provenance {
        Provenance::Known { digest } => digest,
        Provenance::Unknown { reason } => unreachable!("checked above: {reason}"),
        Provenance::AbsentProven { evidence } => unreachable!("not a known digest: {evidence}"),
        Provenance::GuardedExternally { guard } => unreachable!("not a known digest: {guard}"),
    }
}

fn assert_unknown(root: &Path, expected: &str) {
    let provenance = resolve(root);
    assert!(
        matches!(provenance, Provenance::Unknown { ref reason } if reason.contains(expected)),
        "expected unknown {expected}: {provenance:?}"
    );
}

#[test]
fn semantic_inputs_change_identity_regardless_of_filename_class() {
    let temp = checkout();
    let root = temp.path();
    let inputs = [
        ("docs/description.md", "# Compile-time documentation\n"),
        (
            "native/build.c",
            "#include \"fixture.h\"\nint fixture(void) { return FIXTURE; }\n",
        ),
        ("native/fixture.h", "#define FIXTURE 1\n"),
        ("fixtures/response.json", "{\"answer\":1}\n"),
        ("dependency/src/lib.rs", "pub const ANSWER: u8 = 1;\n"),
    ];
    write(
        root,
        "src/lib.rs",
        "#![doc = include_str!(\"../docs/description.md\")]\n\
         pub const FIXTURE: &str = include_str!(\"../fixtures/response.json\");\n",
    );
    write(
        root,
        "build.rs",
        "fn main() { cc::Build::new().file(\"native/build.c\").compile(\"fixture\"); }\n",
    );
    for (path, contents) in inputs {
        write(root, path, contents);
    }
    git(root, &["add", "."]);
    let original = digest(root);
    assert_eq!(digest(root), original, "unchanged inputs remain stable");
    for (path, contents) in inputs {
        write(root, path, &format!("{contents}\n/* changed input */\n"));
        assert_ne!(digest(root), original, "uncommitted edit to {path}");
        write(root, path, contents);
        assert_eq!(digest(root), original, "restored input {path}");
    }
}

#[test]
fn tracked_additions_deletions_and_renames_change_identity() {
    let temp = checkout();
    let root = temp.path();
    let initial = digest(root);
    write(root, "docs/new.md", "included bytes\n");
    git(root, &["add", "docs/new.md"]);
    let added = digest(root);
    assert_ne!(added, initial);
    git(root, &["mv", "docs/new.md", "docs/renamed.md"]);
    let renamed = digest(root);
    assert_ne!(renamed, added, "paths are semantic inputs");
    git(root, &["rm", "-f", "docs/renamed.md"]);
    assert_eq!(
        digest(root),
        initial,
        "deletion restores original inventory"
    );
}

#[test]
fn tracked_file_missing_from_disk_makes_identity_unknown() {
    let temp = checkout();
    std::fs::remove_file(temp.path().join("Cargo.toml")).expect("remove tracked file");
    assert!(matches!(resolve(temp.path()), Provenance::Unknown { .. }));
}

#[test]
fn git_ignored_generated_output_does_not_change_identity() {
    let temp = checkout();
    write(temp.path(), ".gitignore", "target/\n");
    git(temp.path(), &["add", ".gitignore"]);
    let original = digest(temp.path());
    write(
        temp.path(),
        "target/native/generated.h",
        "generated header\n",
    );
    assert_eq!(digest(temp.path()), original);
}

#[test]
fn conflicting_index_and_submodule_records_are_rejected() {
    let object = "0123456789012345678901234567890123456789";
    for stage in ["1", "2", "3"] {
        let record = format!("100644 {object} {stage}\tconflict.rs\0");
        assert_eq!(
            parse_inventory(record.as_bytes()),
            Err("unresolved_index:conflict.rs".to_owned())
        );
    }
    let record = format!("160000 {object} 0\tdependency\0");
    assert_eq!(
        parse_inventory(record.as_bytes()),
        Err("unsupported_checkout_kind:dependency:160000".to_owned())
    );
}

#[test]
fn duplicate_index_records_are_rejected() {
    let record = "100644 0123456789012345678901234567890123456789 0\tfile.rs\0";
    assert_eq!(
        parse_inventory(format!("{record}{record}").as_bytes()),
        Err("duplicate_checkout_path:file.rs".to_owned())
    );
}

#[cfg(unix)]
#[test]
fn replacing_tracked_leaf_with_symlink_makes_identity_unknown() {
    let temp = checkout();
    let outside = TempDir::new().expect("outside checkout");
    write(outside.path(), "Cargo.toml", "external bytes\n");
    std::fs::remove_file(temp.path().join("Cargo.toml")).expect("remove leaf");
    std::os::unix::fs::symlink(
        outside.path().join("Cargo.toml"),
        temp.path().join("Cargo.toml"),
    )
    .expect("symlink leaf");
    assert_unknown(temp.path(), "symlink:Cargo.toml");
}

#[cfg(unix)]
#[test]
fn replacing_tracked_ancestor_with_symlink_makes_identity_unknown() {
    let temp = checkout();
    let outside = TempDir::new().expect("outside checkout");
    write(temp.path(), "native/input.h", "tracked bytes\n");
    git(temp.path(), &["add", "."]);
    write(outside.path(), "input.h", "external bytes\n");
    std::fs::remove_dir_all(temp.path().join("native")).expect("remove ancestor");
    std::os::unix::fs::symlink(outside.path(), temp.path().join("native"))
        .expect("symlink ancestor");
    assert_unknown(temp.path(), "symlink:native");
}

#[cfg(unix)]
#[test]
fn executable_permission_changes_identity_without_index_update() {
    use std::os::unix::fs::PermissionsExt;

    let temp = checkout();
    let path = temp.path().join("Cargo.toml");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("set ordinary mode");
    let original = digest(temp.path());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("set executable mode");
    assert_ne!(digest(temp.path()), original);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("restore ordinary mode");
    assert_eq!(digest(temp.path()), original);
}

#[test]
fn non_git_fixtures_bind_native_bytes_even_under_target() {
    let temp = TempDir::new().expect("non-git fixture");
    let root = temp.path();
    write(root, "src/lib.rs", "pub const ANSWER: u8 = 1;\n");
    write(
        root,
        "target/native/build.c",
        "int answer(void) { return 1; }\n",
    );
    write(root, "target/native/input.h", "#define ANSWER 1\n");
    let original = digest(root);
    assert_eq!(digest(root), original);
    for path in ["target/native/build.c", "target/native/input.h"] {
        let original_bytes = std::fs::read(root.join(path)).expect("original bytes");
        write(root, path, "changed native bytes\n");
        assert_ne!(digest(root), original, "fixture path {path} participates");
        std::fs::write(root.join(path), original_bytes).expect("restore fixture");
        assert_eq!(digest(root), original);
    }
}

#[cfg(unix)]
#[test]
fn non_git_fixture_symlink_is_unknown() {
    let temp = TempDir::new().expect("non-git fixture");
    let outside = TempDir::new().expect("outside fixture");
    write(outside.path(), "input.h", "external bytes\n");
    std::os::unix::fs::symlink(outside.path(), temp.path().join("native"))
        .expect("symlink native directory");
    assert_unknown(temp.path(), "symlink:");
}

#[cfg(unix)]
#[test]
fn non_git_backslash_filename_cannot_alias_nested_input() {
    let temp = TempDir::new().expect("non-git fixture");
    write(temp.path(), "a/b", "nested input\n");
    write(temp.path(), "a\\b", "distinct literal filename\n");
    assert!(matches!(resolve(temp.path()), Provenance::Unknown { .. }));
}

#[test]
fn undeclared_nonignored_include_payload_changes_identity() {
    let temp = checkout();
    write(
        temp.path(),
        "src/lib.rs",
        "pub const DATA: &str = include_str!(\"../payload.txt\");\n",
    );
    git(temp.path(), &["add", "src/lib.rs"]);
    write(temp.path(), "payload.txt", "first payload\n");
    let original = digest(temp.path());
    write(temp.path(), "payload.txt", "second payload\n");
    assert_ne!(digest(temp.path()), original);
}
