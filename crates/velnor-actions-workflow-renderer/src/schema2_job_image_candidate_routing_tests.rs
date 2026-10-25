use super::{WORKFLOW_PATH, request_with_image};

#[test]
fn scale_set_candidate_without_image_uses_all_selector_labels() {
    let request = request_with_image(None);
    let files = crate::schema2::render_schema2_workflows(&request)
        .expect("Render the image candidate on the configured Scale Set.");
    let paths = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            ".github/workflows/product-release.yml",
            ".github/workflows/product-release-images.yml",
            WORKFLOW_PATH,
        ]
    );
    let file = files
        .iter()
        .find(|file| file.path == WORKFLOW_PATH)
        .expect("Render the job image candidate workflow.");
    assert_eq!(file.path, WORKFLOW_PATH);
    let runs_on = format!("runs-on: [{}]", request.scale_set.labels().join(", "));
    assert_eq!(file.bytes.matches(&runs_on).count(), 3);
    assert!(!file.bytes.contains("container:"));
    assert!(file.bytes.contains("$VELNOR_NATIVE_IMAGE_ACTIONS"));
    assert!(file.bytes.contains("image-maintenance build-job-candidate"));
    assert!(file.bytes.contains("--source-commit"));
    assert!(file.bytes.contains("GITHUB_SHA"));
}

#[test]
fn scale_set_candidate_rejects_a_non_linux_amd64_source_runner() {
    let mut request = request_with_image(None);
    request.hosted_label = "ubuntu-26.04-arm".to_owned();
    assert!(crate::schema2::render_schema2_workflows(&request).is_err());
}
