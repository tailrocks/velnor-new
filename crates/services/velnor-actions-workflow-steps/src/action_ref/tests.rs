use super::validate_uses;

#[test]
fn only_canonical_generated_provider_prelude_paths_are_local() {
    assert!(validate_uses("./.github/actions/tofu-provider-prelude-0").is_ok());
    assert!(validate_uses("./.github/actions/tofu-provider-prelude-12").is_ok());
    for uses in [
        "./.github/actions/tofu-provider-prelude-",
        "./.github/actions/tofu-provider-prelude-01",
        "./.github/actions/tofu-provider-prelude-1-extra",
        "./.github/actions/untrusted-local-action",
    ] {
        assert!(
            validate_uses(uses).is_err(),
            "unexpected local action {uses}"
        );
    }
}
