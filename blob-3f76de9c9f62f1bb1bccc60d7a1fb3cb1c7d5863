use super::*;

#[test]
fn consumer_manifest_generator_tag_matches_source_commit() {
    let version = env!("CARGO_PKG_VERSION");
    let commit = "a".repeat(40);
    let version_tag = format!("/download/v{version}/");
    let generator_tag = format!("/download/generator-{commit}/");
    let valid = test_manifest_json().replace(&version_tag, &generator_tag);
    assert!(
        valid.contains(&generator_tag),
        "fixture must use generator tag"
    );
    assert!(consumer_acquire_from("ubuntu-26.04", version, Some(&valid)).is_ok());

    let wrong_commit = "b".repeat(40);
    let wrong_tag = format!("/download/generator-{wrong_commit}/");
    let mismatched = valid.replace(&generator_tag, &wrong_tag);
    assert!(
        consumer_acquire_from("ubuntu-26.04", version, Some(&mismatched)).is_err(),
        "generator tag must bind to manifest source commit"
    );
}
