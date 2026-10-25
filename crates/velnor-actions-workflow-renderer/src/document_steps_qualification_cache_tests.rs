use std::collections::BTreeMap;

use super::cache_env::validate_qualification_cache_env;

#[test]
fn qualification_cache_operations_restrict_explicit_environment() {
    let owner = BTreeMap::from([
        (
            "VELNOR_QUALIFICATION_CACHE_OWNER_ID".to_owned(),
            "rust-package".to_owned(),
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_LAYER".to_owned(),
            "cargo_sources".to_owned(),
        ),
        (
            "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE".to_owned(),
            "${{ steps.qualification-runtime-identity.outputs.runtime_identity_path }}".to_owned(),
        ),
    ]);
    assert!(validate_qualification_cache_env("bind-qualification-cache-v1", &owner).is_ok());

    let token = BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]);
    assert!(
        validate_qualification_cache_env("observe-qualification-cache-backend-v1", &token).is_ok()
    );
    assert!(
        validate_qualification_cache_env("export-qualification-cache-payload-v1", &token).is_err()
    );

    let arbitrary = BTreeMap::from([(
        "GH_TOKEN".to_owned(),
        "${{ secrets.MANAGEMENT }}".to_owned(),
    )]);
    assert!(
        validate_qualification_cache_env("observe-qualification-cache-backend-v1", &arbitrary)
            .is_err()
    );

    let unrelated = BTreeMap::from([("CARGO_HOME".to_owned(), "/tmp/cargo".to_owned())]);
    assert!(validate_qualification_cache_env("bind-qualification-cache-v1", &unrelated).is_err());
}

#[test]
fn github_token_expression_is_scoped_to_backend_observation() {
    let token = BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]);
    assert!(crate::commands::validate_env(&token).is_err());
    assert!(crate::commands::validate_qualification_cache_env(&token).is_ok());
    assert!(
        validate_qualification_cache_env("observe-qualification-cache-backend-v1", &token).is_ok()
    );
    assert!(
        validate_qualification_cache_env("export-qualification-cache-payload-v1", &token).is_err()
    );

    let arbitrary = BTreeMap::from([(
        "VELNOR_QUALIFICATION_CACHE_KEY".to_owned(),
        "${{ github.sha }}".to_owned(),
    )]);
    assert!(crate::commands::validate_qualification_cache_env(&arbitrary).is_err());
}
