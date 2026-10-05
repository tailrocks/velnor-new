//! Canonical workspace and relative paths for Cargo source archives.
use super::*;

#[test]
fn sources_subset_accepts_canonical_owned_home_paths_only() {
    let sha = "b".repeat(40);
    let uses = format!("actions/cache/restore@{sha}");
    let home = "${{ github.workspace }}/.velnor/cache/cargo";
    let expression_paths = [
        format!("{home}/registry/cache"),
        format!("{home}/registry/index"),
        format!("{home}/git/db"),
    ];
    assert!(
        cache_action_step(true, &uses, "sources", "k", &[], &expression_paths).is_ok(),
        "canonical workspace paths must parse"
    );
    let relative_paths = [
        ".velnor/cache/cargo/registry/cache".to_owned(),
        ".velnor/cache/cargo/registry/index".to_owned(),
        ".velnor/cache/cargo/git/db".to_owned(),
    ];
    assert!(
        cache_action_step(true, &uses, "sources", "k", &[], &relative_paths).is_ok(),
        "workspace-relative producer paths remain valid"
    );
    for bad in [
        format!("{home}/registry/src/x"),
        format!("{home}/credentials.toml"),
        format!("{home}/.crates.toml"),
        format!("{home}/.crates2.json"),
        format!("{home}/bin"),
        format!("{home}/bin/cargo-nextest"),
        format!("{home}/../escape"),
        "~/.cargo/registry/cache".to_owned(),
    ] {
        assert!(
            cache_action_step(true, &uses, "sources", "k", &[], std::slice::from_ref(&bad))
                .is_err(),
            "must reject {bad}"
        );
    }
}
