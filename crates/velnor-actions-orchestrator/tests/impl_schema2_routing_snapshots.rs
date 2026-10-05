use super::*;

#[test]
fn schema2_workflows_match_expected_bytes() -> TestResult {
    let repo = make_repo(&workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification = required_file(&tree, ".github/workflows/qualification.yml")?;
    assert_eq!(
        qualification,
        &marked(schema2_feature_snapshots::QUALIFICATION)
    );
    let image = required_file(&tree, ".github/workflows/image-release.yml")?;
    let macos = required_file(&tree, ".github/workflows/macos-binary-release.yml")?;
    assert_eq!(image, &marked(schema2_release_snapshots::IMAGE_RELEASE));
    assert_eq!(macos, &marked(schema2_release_snapshots::MACOS_RELEASE));
    assert_image_producer(image)?;
    assert_macos_producer(macos)?;
    assert_eq!(
        required_file(&tree, ".github/workflows/monitoring.yml")?,
        &marked(MONITORING)
    );
    schema2_generator_release_snapshots::assert_rendered(&tree)?;
    schema2_generator_candidate_snapshots::assert_rendered(&tree)?;
    Ok(())
}
