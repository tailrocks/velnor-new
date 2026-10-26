use std::error::Error;

#[path = "api2026_pull_context_fixture.rs"]
mod fixture;

#[test]
fn absent_rest_merge_sha_reaches_owned_locator_gate() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::Valid,
        "homebrew_authority_locator_missing",
    )
}

#[test]
fn reversed_api_merge_parents_are_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ReversedParents,
        "package_public_pull_request_merge_commit_mismatch",
    )
}

#[test]
fn changed_merge_ref_is_rejected() -> Result<(), Box<dyn Error>> {
    fixture::run(
        fixture::Scenario::ChangedRef,
        "package_public_pull_request_merge_ref_changed",
    )
}
