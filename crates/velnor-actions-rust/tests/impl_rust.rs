//! File-index builder cases.
use crate::support::{Outcome, TempDir};
use velnor_actions_rust::{
    IndexError, build_index, discover_candidates, is_excluded, matches_glob, validate_pattern,
};

#[test]
fn stack_id_is_rust() {
    assert_eq!(velnor_actions_rust::STACK_ID, "rust");
}

#[test]
fn index_builds_sorted_posix_paths() -> Outcome {
    let dir = TempDir::create("index-sorted")?;
    dir.write("b/Cargo.toml", "[package]\n")?;
    dir.write("a/lib.rs", "fn a() {}\n")?;
    dir.write("Cargo.toml", "[workspace]\n")?;
    let index = build_index(dir.path(), &[])?;
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
    let result = build_index(dir.path(), &[]);
    assert!(matches!(result, Err(IndexError::SymlinkEscape(_))));
    Ok(())
}

#[cfg(unix)]
#[test]
fn index_refuses_symlink_loop() -> Outcome {
    let dir = TempDir::create("index-loop")?;
    std::os::unix::fs::symlink(dir.path(), dir.path().join("loop"))?;
    let result = build_index(dir.path(), &[]);
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
    let index = build_index(dir.path(), &[])?;
    assert!(index.contains("real/data.txt"));
    assert!(index.contains("alias.txt"));
    Ok(())
}
