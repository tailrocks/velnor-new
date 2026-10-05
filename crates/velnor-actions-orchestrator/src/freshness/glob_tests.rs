use super::{component_matches, matching_entries, matching_entries_with_limit};

#[test]
fn glob_components_support_classes_negation_ranges_and_literal_brackets() {
    assert!(component_matches("pins[ab].rs", "pinsa.rs"));
    assert!(!component_matches("pins[ab].rs", "pinsc.rs"));
    assert!(component_matches("pins[!a].rs", "pinsb.rs"));
    assert!(!component_matches("pins[!a].rs", "pinsa.rs"));
    assert!(component_matches("pins[a-c].rs", "pinsb.rs"));
    assert!(component_matches("literal[[]x.rs", "literal[x.rs"));
    assert!(component_matches("literal[]]x.rs", "literal]x.rs"));
    assert!(component_matches("café?.rs", "caféλ.rs"));
    assert!(!component_matches("*.rs", ".hidden.rs"));
    assert!(component_matches(".*.rs", ".hidden.rs"));
}

#[cfg(unix)]
#[test]
fn wildcard_matches_symlink_to_regular_file() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("real.rs"), "source")?;
    std::os::unix::fs::symlink("real.rs", root.path().join("alias.rs"))?;
    let entries = matching_entries(root.path(), "*.rs")?;
    assert!(
        entries
            .iter()
            .any(|entry| entry.path == "alias.rs" && entry.is_file)
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn directory_symlinks_cannot_escape_root_but_internal_links_match()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let external = tempfile::tempdir()?;
    std::fs::write(root.path().join("nested.rs"), "source")?;
    std::fs::write(external.path().join("outside.rs"), "outside")?;
    std::os::unix::fs::symlink(external.path(), root.path().join("outside-link"))?;
    let direct_error = matching_entries(root.path(), "*/outside.rs")
        .expect_err("wildcard traversal must reject links outside the repository");
    assert!(direct_error.contains("escapes repository root"));
    let recursive_error = matching_entries(root.path(), "**/*.rs")
        .expect_err("recursive traversal must reject links outside the repository");
    assert!(recursive_error.contains("escapes repository root"));

    std::fs::create_dir_all(root.path().join("nested"))?;
    std::fs::write(root.path().join("nested/internal.rs"), "source")?;
    std::os::unix::fs::symlink("nested", root.path().join("internal-link"))?;
    let internal = matching_entries(root.path(), "internal-link/*.rs")?;
    assert!(
        internal
            .iter()
            .any(|entry| entry.path == "internal-link/internal.rs" && entry.is_file)
    );
    Ok(())
}

#[test]
fn filesystem_globs_apply_bracket_classes_and_negation() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    for name in ["pin_a.rs", "pin_b.rs", "pin_c.rs"] {
        std::fs::write(root.path().join(name), "source")?;
    }
    let selected = matching_entries(root.path(), "pin_[ab].rs")?;
    let excluded = matching_entries(root.path(), "pin_[!a].rs")?;
    assert_eq!(selected.len(), 2);
    assert_eq!(excluded.len(), 2);
    assert!(selected.iter().any(|entry| entry.path == "pin_a.rs"));
    assert!(!selected.iter().any(|entry| entry.path == "pin_c.rs"));
    assert!(!excluded.iter().any(|entry| entry.path == "pin_a.rs"));
    assert!(excluded.iter().any(|entry| entry.path == "pin_c.rs"));
    Ok(())
}

#[test]
fn static_prefix_prunes_unrelated_trees_and_recursive_work_is_capped()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let wanted = root.path().join("crates/active/src/core");
    let unrelated = root.path().join("target/unrelated");
    std::fs::create_dir_all(&wanted)?;
    std::fs::create_dir_all(&unrelated)?;
    std::fs::write(wanted.join("module.rs"), "source")?;
    for index in 0..40 {
        std::fs::write(unrelated.join(format!("file-{index}.txt")), "noise")?;
    }
    let entries = matching_entries_with_limit(root.path(), "crates/active/src/**/*.rs", 16)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "crates/active/src/core/module.rs");
    assert!(
        matching_entries_with_limit(root.path(), "**/*.rs", 16).is_err(),
        "wide recursive glob must stop at its traversal limit"
    );
    Ok(())
}

#[test]
fn static_file_pattern_matches_without_directory_walk() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::create_dir_all(root.path().join("target/large"))?;
    std::fs::write(root.path().join("target/large/only.rs"), "source")?;
    std::fs::write(root.path().join("chosen.rs"), "source")?;
    let entries = matching_entries(root.path(), "chosen.rs")?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "chosen.rs");
    Ok(())
}

#[test]
fn recursive_double_star_terminal_matches_nested_files_and_directories()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::create_dir_all(root.path().join("visible/nested"))?;
    std::fs::write(root.path().join("top.rs"), "source")?;
    std::fs::write(root.path().join("visible/nested/deep.rs"), "source")?;
    std::fs::write(root.path().join(".hidden.rs"), "hidden")?;
    let entries = matching_entries(root.path(), "**")?;
    assert!(entries.iter().any(|entry| entry.path == "top.rs"));
    assert!(entries.iter().any(|entry| entry.path == "visible"));
    assert!(
        entries
            .iter()
            .any(|entry| entry.path == "visible/nested/deep.rs")
    );
    assert!(!entries.iter().any(|entry| entry.path == ".hidden.rs"));
    Ok(())
}

#[test]
fn glob_rejects_root_escape_and_absolute_patterns() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    assert!(matching_entries(root.path(), "../outside/*.rs").is_err());
    assert!(matching_entries(root.path(), "/outside/*.rs").is_err());
    Ok(())
}
