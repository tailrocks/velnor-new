//! File-index builder cases.
use crate::support::{Outcome, TempDir};
use velnor_actions_contract::Stack;
use velnor_actions_contract_planning::{
    IndexError, build_index, build_index_from_list, build_index_walk, is_excluded, matches_glob,
    validate_pattern,
};
use velnor_actions_rust::discover_candidates;

#[test]
fn stack_id_is_rust() {
    assert_eq!(Stack::Rust.id(), "rust");
}

#[test]
fn index_builds_sorted_posix_paths() -> Outcome {
    let dir = TempDir::create("index-sorted")?;
    dir.write("b/Cargo.toml", "[package]\n")?;
    dir.write("a/lib.rs", "fn a() {}\n")?;
    dir.write("Cargo.toml", "[workspace]\n")?;
    let index = build_index_walk(dir.path(), &[])?;
    let files: Vec<&str> = index.files().iter().map(String::as_str).collect();
    assert_eq!(files, vec!["Cargo.toml", "a/lib.rs", "b/Cargo.toml"]);
    assert_eq!(index.len(), 3);
    assert!(!index.is_empty());
    assert!(index.contains("a/lib.rs"));
    assert!(!index.contains("missing.rs"));
    Ok(())
}

#[test]
fn index_applies_exclusions_before_detection() -> Outcome {
    let dir = TempDir::create("index-exclude")?;
    dir.write("Cargo.toml", "[workspace]\n")?;
    dir.write("vendor/dep/Cargo.toml", "[package]\n")?;
    dir.write("fixtures/sample/Cargo.toml", "[package]\n")?;
    let exclusions = vec!["vendor/**".to_owned(), "fixtures/**".to_owned()];
    let index = build_index(dir.path(), &exclusions)?;
    assert!(index.contains("Cargo.toml"));
    assert!(!index.contains("vendor/dep/Cargo.toml"));
    assert!(!index.contains("fixtures/sample/Cargo.toml"));
    let candidates = discover_candidates(&index);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].manifest, "Cargo.toml");
    Ok(())
}

#[test]
fn index_applies_builtin_git_exclusion() -> Outcome {
    let dir = TempDir::create("index-git")?;
    dir.write(".git/HEAD", "ref: refs/heads/main\n")?;
    dir.write("src/main.rs", "fn main() {}\n")?;
    let index = build_index(dir.path(), &[])?;
    assert!(!index.contains(".git/HEAD"));
    assert!(index.contains("src/main.rs"));
    Ok(())
}

#[test]
fn index_rejects_malformed_patterns() {
    for pattern in ["", "/absolute", "../escape", "a/../b", "bad|pipe"] {
        assert!(
            matches!(
                validate_pattern(pattern),
                Err(IndexError::MalformedPattern(_))
            ),
            "pattern must be rejected: {pattern}"
        );
    }
    assert!(validate_pattern("vendor/**").is_ok());
    assert!(validate_pattern("crates/*/target").is_ok());
}

#[test]
fn glob_matcher_covers_classes_and_braces() {
    assert!(matches_glob("vendor/**", "vendor/dep/Cargo.toml"));
    assert!(matches_glob("**/Cargo.toml", "crates/a/Cargo.toml"));
    assert!(matches_glob("crates/*/Cargo.toml", "crates/a/Cargo.toml"));
    assert!(!matches_glob(
        "crates/*/Cargo.toml",
        "crates/a/b/Cargo.toml"
    ));
    assert!(matches_glob("src/?.rs", "src/a.rs"));
    assert!(!matches_glob("src/?.rs", "src/ab.rs"));
    assert!(matches_glob("src/[a-c].rs", "src/b.rs"));
    assert!(!matches_glob("src/[a-c].rs", "src/d.rs"));
    assert!(matches_glob("src/[!a-c].rs", "src/d.rs"));
    assert!(matches_glob("{src,tests}/main.rs", "tests/main.rs"));
    assert!(!matches_glob("{src,tests}/main.rs", "docs/main.rs"));
    assert!(!matches_glob("*.toml", "crates/a/Cargo.toml"));
}

#[test]
fn exclusion_matches_ancestor_directories() {
    assert!(is_excluded("vendor/a/b.rs", &["vendor"]));
    assert!(is_excluded("vendor/a/b.rs", &["vendor/**"]));
    assert!(!is_excluded("src/main.rs", &["vendor/**"]));
    assert!(is_excluded("target/debug/a", &["target/**"]));
}

#[test]
fn index_skips_non_file_types() -> Outcome {
    let dir = TempDir::create("index-types")?;
    dir.write("Cargo.toml", "[workspace]\n")?;
    std::fs::create_dir_all(dir.path().join("empty_dir"))?;
    let index = build_index(dir.path(), &[])?;
    assert_eq!(index.len(), 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn index_refuses_symlink_escape() -> Outcome {
    let dir = TempDir::create("index-escape")?;
    let outside = TempDir::create("index-outside")?;
    outside.write("secret.txt", "secret\n")?;
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape"))?;
    let result = build_index_walk(dir.path(), &[]);
    assert!(matches!(result, Err(IndexError::SymlinkEscape(_))));
    Ok(())
}

#[cfg(unix)]
#[test]
fn index_refuses_symlink_loop() -> Outcome {
    let dir = TempDir::create("index-loop")?;
    std::os::unix::fs::symlink(dir.path(), dir.path().join("loop"))?;
    let result = build_index_walk(dir.path(), &[]);
    assert!(matches!(result, Err(IndexError::SymlinkLoop(_))));
    Ok(())
}

#[cfg(unix)]
#[test]
fn index_follows_internal_file_symlink() -> Outcome {
    let dir = TempDir::create("index-link")?;
    dir.write("real/data.txt", "data\n")?;
    std::os::unix::fs::symlink(
        dir.path().join("real/data.txt"),
        dir.path().join("alias.txt"),
    )?;
    let index = build_index_walk(dir.path(), &[])?;
    assert!(index.contains("real/data.txt"));
    assert!(index.contains("alias.txt"));
    Ok(())
}

#[test]
fn index_from_list_filters_sorts_and_dedupes() -> Outcome {
    let dir = TempDir::create("index-from-list")?;
    let files = vec![
        "b/Cargo.toml".to_owned(),
        "a/lib.rs".to_owned(),
        "a/lib.rs".to_owned(),
        "vendor/dep/Cargo.toml".to_owned(),
        ".git/HEAD".to_owned(),
    ];
    let index = build_index_from_list(dir.path(), &files, &["vendor/**".to_owned()])?;
    let listed: Vec<&str> = index.files().iter().map(String::as_str).collect();
    assert_eq!(listed, vec!["a/lib.rs", "b/Cargo.toml"]);
    assert_eq!(index.root(), dir.path().canonicalize()?);
    Ok(())
}

#[test]
fn index_from_list_rejects_escaping_entries() {
    let root = std::env::temp_dir();
    for entry in ["", "/absolute/path.rs", "../escape.rs", "a/../../escape.rs"] {
        let files = vec![entry.to_owned()];
        assert!(
            matches!(
                build_index_from_list(&root, &files, &[]),
                Err(IndexError::SymlinkEscape(_))
            ),
            "entry must be rejected: {entry}"
        );
    }
}

#[test]
fn index_walk_matches_from_list_on_same_tree() -> Outcome {
    let dir = TempDir::create("index-walk-parity")?;
    dir.write("Cargo.toml", "[workspace]\n")?;
    dir.write("crates/a/Cargo.toml", "[package]\n")?;
    let walked = build_index_walk(dir.path(), &[])?;
    let from_list = build_index_from_list(dir.path(), walked.files(), &[])?;
    assert_eq!(walked.files(), from_list.files());
    assert_eq!(walked.root(), from_list.root());
    Ok(())
}
