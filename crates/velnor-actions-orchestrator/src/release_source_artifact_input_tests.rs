use super::*;

#[test]
fn original_source_fields_reference_only_fixed_producer_outputs() {
    let environment = source_input_environment();
    assert_eq!(environment.len(), 9);
    for (key, output) in [
        (
            "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_ID",
            "source-snapshot-artifact-id",
        ),
        (
            "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_DIGEST",
            "source-snapshot-artifact-digest",
        ),
        (
            "RELEASE_SOURCE_SNAPSHOT_BLOB_SHA256",
            "source-snapshot-blob-sha256",
        ),
        ("RELEASE_SOURCE_COMMIT_SHA", "source-commit-sha"),
        ("RELEASE_SOURCE_TREE_SHA", "source-tree-sha"),
    ] {
        assert_eq!(environment[key], needs(output));
    }
    assert_eq!(environment["GITHUB_REF"], "${{ github.ref }}");
    assert_eq!(
        environment["GITHUB_WORKFLOW_REF"],
        "${{ github.workflow_ref }}"
    );
    assert_eq!(
        environment["GITHUB_WORKFLOW_SHA"],
        "${{ github.workflow_sha }}"
    );
    assert_eq!(environment["RUNNER_TEMP"], "${{ runner.temp }}");
}
