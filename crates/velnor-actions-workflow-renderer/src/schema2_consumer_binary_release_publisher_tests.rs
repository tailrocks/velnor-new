#[path = "schema2_consumer_binary_release_publisher_stub.rs"]
mod stub;
#[path = "schema2_consumer_binary_release_publisher_support.rs"]
mod support;

#[test]
fn publisher_gates_mutation_and_verifies_release_postconditions() {
    for scenario in [
        "success",
        "immutable-disabled",
        "environment-unprotected",
        "environment-missing",
        "missing-token",
        "stale-before-tag",
        "stale-before-publish",
        "tag-collision",
        "partial-collision",
        "non-404",
        "mutable-release",
        "extra-release-asset",
        "download-extra-file",
        "download-corrupt-bytes",
        "download-extra-checksum",
        "final-attestation-failed",
    ] {
        support::run_publish_scenario(scenario);
    }
}
