//! Branch shorthand parsing from the local `origin/HEAD` symbolic ref.

use super::branch_from_origin_head;

#[test]
fn origin_head_accepts_only_supported_branch_names() {
    for (input, expected) in [
        ("origin/main", "main"),
        ("origin/release/1.2", "release/1.2"),
        ("feature/x_y-z", "feature/x_y-z"),
        ("refs/heads/main", "refs/heads/main"),
    ] {
        assert_eq!(branch_from_origin_head(input).as_deref(), Some(expected));
    }
    for input in [
        "origin/HEAD",
        "origin/-main",
        "origin/a/../b",
        "origin/main;evil",
        "origin/main\non: [push]",
    ] {
        assert_eq!(branch_from_origin_head(input), None, "{input:?}");
    }
}
