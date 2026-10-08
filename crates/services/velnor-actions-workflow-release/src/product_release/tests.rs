use super::{ProductReleaseFamily, ProductReleaseSpec};

#[test]
fn selection_is_nonempty() {
    assert!(
        ProductReleaseSpec::new([])
            .is_err_and(|error| { error.to_string().contains("product_release_families_empty") })
    );
}

#[test]
fn selection_rejects_duplicate_families() {
    assert!(
        ProductReleaseSpec::new([ProductReleaseFamily::Images, ProductReleaseFamily::Images,])
            .is_err_and(|error| {
                error
                    .to_string()
                    .contains("product_release_family_duplicate:images")
            })
    );
}

#[test]
fn selection_has_stable_order_and_membership() {
    let spec = ProductReleaseSpec::new([
        ProductReleaseFamily::Generator,
        ProductReleaseFamily::Images,
        ProductReleaseFamily::Binary,
    ])
    .expect("valid family selection");

    assert_eq!(
        spec.families(),
        &[
            ProductReleaseFamily::Images,
            ProductReleaseFamily::Binary,
            ProductReleaseFamily::Generator,
        ]
    );
    assert!(spec.includes(ProductReleaseFamily::Images));
    assert!(spec.includes(ProductReleaseFamily::Generator));
}

#[test]
fn families_keep_the_release_workflow_contract() {
    assert_eq!(
        ProductReleaseFamily::Images.workflow_path(),
        ".github/workflows/product-release-images.yml"
    );
    assert_eq!(
        ProductReleaseFamily::Binary.prepare_job_id(),
        "prepare-binary"
    );
    assert_eq!(
        ProductReleaseFamily::Generator.call_job_id(),
        "release-generator"
    );
    assert_eq!(
        ProductReleaseFamily::Generator.label(),
        "velnor-actions generator"
    );
}
