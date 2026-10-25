use super::{GithubTree, GithubTreeEntry, valid_timestamp, validate_single_formula_delta};

fn tree(formula_sha: &str, extra_sha: &str) -> GithubTree {
    GithubTree {
        sha: "a".repeat(40),
        truncated: false,
        tree: vec![
            GithubTreeEntry {
                path: "Formula".to_owned(),
                mode: "040000".to_owned(),
                object_type: "tree".to_owned(),
                sha: formula_sha[..1].repeat(40),
            },
            GithubTreeEntry {
                path: "Formula/example.rb".to_owned(),
                mode: "100644".to_owned(),
                object_type: "blob".to_owned(),
                sha: formula_sha.to_owned(),
            },
            GithubTreeEntry {
                path: "README.md".to_owned(),
                mode: "100644".to_owned(),
                object_type: "blob".to_owned(),
                sha: extra_sha.to_owned(),
            },
        ],
    }
}

fn nested_tree(
    formula_sha: &str,
    root_sha: &str,
    nested_sha: &str,
    readme_sha: &str,
) -> GithubTree {
    GithubTree {
        sha: "a".repeat(40),
        truncated: false,
        tree: vec![
            GithubTreeEntry {
                path: "Formula".to_owned(),
                mode: "040000".to_owned(),
                object_type: "tree".to_owned(),
                sha: root_sha.to_owned(),
            },
            GithubTreeEntry {
                path: "Formula/nested".to_owned(),
                mode: "040000".to_owned(),
                object_type: "tree".to_owned(),
                sha: nested_sha.to_owned(),
            },
            GithubTreeEntry {
                path: "Formula/nested/example.rb".to_owned(),
                mode: "100644".to_owned(),
                object_type: "blob".to_owned(),
                sha: formula_sha.to_owned(),
            },
            GithubTreeEntry {
                path: "README.md".to_owned(),
                mode: "100644".to_owned(),
                object_type: "blob".to_owned(),
                sha: readme_sha.to_owned(),
            },
        ],
    }
}

#[test]
fn historical_tree_rejects_unchanged_symlink_and_submodule_entries() {
    for (mode, kind) in [("120000", "blob"), ("160000", "commit")] {
        let mut base = tree(&"b".repeat(40), &"c".repeat(40));
        let mut head = tree(&"d".repeat(40), &"c".repeat(40));
        for candidate in [&mut base, &mut head] {
            candidate.tree.push(GithubTreeEntry {
                path: "vendor/object".to_owned(),
                mode: mode.to_owned(),
                object_type: kind.to_owned(),
                sha: "f".repeat(40),
            });
        }
        assert!(validate_single_formula_delta("Formula/example.rb", &base, &head).is_err());
    }
}

#[test]
fn historical_tree_allows_only_formula_ancestor_tree_sha_changes() {
    let base = nested_tree(
        &"b".repeat(40),
        &"c".repeat(40),
        &"d".repeat(40),
        &"e".repeat(40),
    );
    let head = nested_tree(
        &"f".repeat(40),
        &"1".repeat(40),
        &"2".repeat(40),
        &"e".repeat(40),
    );
    assert!(validate_single_formula_delta("Formula/nested/example.rb", &base, &head).is_ok());

    let unrelated_change = nested_tree(
        &"f".repeat(40),
        &"1".repeat(40),
        &"2".repeat(40),
        &"3".repeat(40),
    );
    assert!(
        validate_single_formula_delta("Formula/nested/example.rb", &base, &unrelated_change)
            .is_err()
    );
}

#[test]
fn historical_tree_requires_only_the_formula_blob_to_change() {
    let base = tree(&"b".repeat(40), &"c".repeat(40));
    let changed_formula = tree(&"d".repeat(40), &"c".repeat(40));
    let changed_readme = tree(&"d".repeat(40), &"e".repeat(40));
    assert!(validate_single_formula_delta("Formula/example.rb", &base, &changed_formula).is_ok());
    assert!(validate_single_formula_delta("Formula/example.rb", &base, &changed_readme).is_err());
}

#[test]
fn historical_tree_requires_regular_formula_blobs() {
    let base = tree(&"b".repeat(40), &"c".repeat(40));
    let mut head = tree(&"d".repeat(40), &"c".repeat(40));
    head.tree[0].mode = "100755".to_owned();
    assert!(validate_single_formula_delta("Formula/example.rb", &base, &head).is_err());
}

#[test]
fn historical_merge_timestamp_requires_valid_utc_rfc3339_shape() {
    assert!(valid_timestamp("2024-02-29T23:59:59.123456Z"));
    assert!(!valid_timestamp("2023-02-29T23:59:59Z"));
    assert!(!valid_timestamp("2024-13-01T00:00:00Z"));
    assert!(!valid_timestamp("2024-01-01T24:00:00Z"));
    assert!(!valid_timestamp("2024-01-01T00:00:00+00:00"));
}

#[path = "tap_writer_history_protocol_tests.rs"]
mod protocol;
