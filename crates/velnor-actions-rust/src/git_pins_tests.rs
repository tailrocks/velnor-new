//! Cases for the private git source pin check.

use super::{UnpinnedGitSource, unpinned_git_sources};

fn metadata(sources: &[(&str, Option<&str>)]) -> String {
    let packages: Vec<String> = sources
        .iter()
        .map(|(name, source)| match source {
            Some(source) => format!("{{\"name\":\"{name}\",\"source\":\"{source}\"}}"),
            None => format!("{{\"name\":\"{name}\",\"source\":null}}"),
        })
        .collect();
    format!("{{\"packages\":[{}]}}", packages.join(","))
}

fn check(sources: &[(&str, Option<&str>)]) -> Vec<(String, String)> {
    unpinned_git_sources(&metadata(sources), "ChainArgos")
        .unwrap_or_default()
        .into_iter()
        .map(|UnpinnedGitSource { package, source }| (package, source))
        .collect()
}

#[test]
fn tagged_owner_sources_pass() {
    assert_eq!(
        check(&[
            (
                "a",
                Some("git+https://github.com/ChainArgos/a?tag=a-v1.0.0#0123")
            ),
            (
                "b",
                Some("git+https://github.com/chainargos/b.git?tag=b-v2.1.0#0123")
            ),
        ]),
        Vec::<(String, String)>::new()
    );
}

#[test]
fn branch_rev_and_bare_owner_sources_are_unpinned() {
    let found = check(&[
        (
            "branch",
            Some("git+https://github.com/ChainArgos/x?branch=main#abcd"),
        ),
        (
            "rev",
            Some("git+https://github.com/ChainArgos/y?rev=abcd#abcd"),
        ),
        ("bare", Some("git+https://github.com/ChainArgos/z#abcd")),
        (
            "empty",
            Some("git+https://github.com/ChainArgos/w?tag=#abcd"),
        ),
        (
            "twice",
            Some("git+ssh://git@github.com/ChainArgos/v?tag=a&tag=b#abcd"),
        ),
    ]);
    let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["bare", "branch", "empty", "rev", "twice"]);
    assert_eq!(found[0].1, "https://github.com/ChainArgos/z");
}

#[test]
fn foreign_registry_and_path_sources_are_ignored() {
    assert_eq!(
        check(&[
            (
                "other",
                Some("git+https://github.com/serde-rs/serde?branch=master#abcd")
            ),
            (
                "lookalike",
                Some("git+https://github.com/ChainArgosX/q#abcd")
            ),
            (
                "reg",
                Some("registry+https://github.com/rust-lang/crates.io-index")
            ),
            ("local", None),
        ]),
        Vec::<(String, String)>::new()
    );
}

#[test]
fn malformed_metadata_is_an_error() {
    assert!(unpinned_git_sources("{", "ChainArgos").is_err());
}
