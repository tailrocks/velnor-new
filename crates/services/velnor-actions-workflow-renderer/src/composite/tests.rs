use super::shared_call;

#[test]
fn shared_calls_use_only_canonical_repository_local_actions() {
    let yaml = crate::yaml::render_yaml(
        &shared_call("./.github/actions/rust-0").expect("canonical local action"),
    );
    assert!(
        yaml.contains("uses: ./.github/actions/rust-0 # zizmor: ignore[self-repository]"),
        "{yaml}"
    );
    for uses in [
        "./.github/actions/",
        "./.github/actions/../rust-0",
        "./.github/actions/a/b",
        "./.github/actions/rust.0",
        "./.github/actions/rust-0@deadbeef",
        "actions/checkout@0000000000000000000000000000000000000000",
    ] {
        assert!(shared_call(uses).is_err(), "accepted {uses}");
    }
}
