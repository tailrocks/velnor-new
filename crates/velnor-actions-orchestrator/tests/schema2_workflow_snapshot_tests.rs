use super::*;

#[test]
fn schema2_workflows_match_expected_bytes() -> TestResult {
    let repo = make_repo(&workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    schema2_snapshot_capture::capture_if_requested(&tree)?;
    let qualification = required_file(&tree, ".github/workflows/qualification.yml")?;
    assert_eq!(
        qualification,
        &marked(schema2_feature_snapshots::QUALIFICATION)
    );
    required_file(&tree, ".github/workflows/product-release.yml")?;
    schema2_product_release_snapshots::assert_rendered(&tree)?;
    let images = required_file(&tree, ".github/workflows/product-release-images.yml")?;
    let binary = required_file(&tree, ".github/workflows/product-release-binary.yml")?;
    assert_image_producer(images)?;
    assert_macos_producer(binary)?;
    assert_eq!(
        required_file(&tree, ".github/workflows/monitoring.yml")?,
        &marked(MONITORING)
    );
    Ok(())
}

#[test]
fn committed_release_files_match_schema2_bytes() -> TestResult {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = render_staged_tree(&prepare(&root)?)?;
    schema2_product_release_snapshots::assert_committed(&root, &tree)?;
    Ok(())
}
