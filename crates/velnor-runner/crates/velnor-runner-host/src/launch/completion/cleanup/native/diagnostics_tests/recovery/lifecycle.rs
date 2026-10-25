use super::*;

#[path = "lifecycle/support.rs"]
mod support;

#[test]
fn failed_completion_holds_capacity_until_native_cleanup_retry_settles() -> Result<(), String> {
    super::super::run_serialized_diagnostic_test(
        "failed_completion_holds_capacity_until_native_cleanup_retry_settles",
        || {
            Box::pin(async {
                Box::pin(failed_completion_holds_capacity_until_cleanup_retry_settles())
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn failed_completion_holds_capacity_until_cleanup_retry_settles() -> Result<(), HostError> {
    let state = support::LifecycleFixture::prepare()
        .await
        .map_err(|error| support::stage_error("fixture preparation", error))?;
    support::assert_competing_request_at_capacity(&state, 'd')
        .await
        .map_err(|error| support::stage_error("initial competing admission", error))?;
    let generation = support::fail_first_cleanup(&state)
        .await
        .map_err(|error| support::stage_error("first cleanup failure", error))?;
    support::retry_cleanup(&state, generation)
        .await
        .map_err(|error| support::stage_error("cleanup retry", error))?;
    support::assert_competing_request_admitted(&state, 'f')
        .await
        .map_err(|error| support::stage_error("competing admission after release", error))
}
