use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{ExecutionMode, RoutingWorkflow};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{
    validate_generated_workflow_calls, validate_generated_workflow_calls_with_overrides,
};

use super::{extra_files, workflow_routing_policy};

const CONFIG: &str = r#"
schema = 2
[workflow]
name = "CI"
[execution]
default_profile = "hosted"
mode = "scale-set"
hosted_profile = "hosted"
scale_set_profile = "local"
workflows = ["image_release", "macos_binary_release", "generator_release"]
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
"#;

#[test]
fn emitted_product_and_adoption_workflows_receive_exact_hosted_classifications() {
    let config = crate::config::load_config_from_text(CONFIG).expect("valid scale-set config");
    let catalog = ToolCatalog::pinned();
    let policy = workflow_routing_policy(&config, None).expect("configured routing policy");
    assert!(policy.is_scale_set());
    let (files, final_policy, overrides) =
        extra_files(&config, &catalog, env!("CARGO_PKG_VERSION"), &policy)
            .expect("schema 2 release workflows render");
    let actual = overrides
        .hosted_only_paths()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let expected = [
        ".github/workflows/product-release.yml",
        ".github/workflows/product-release-images.yml",
        ".github/workflows/product-release-binary.yml",
        ".github/workflows/product-release-generator.yml",
        ".github/workflows/homebrew-adoption.yml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
    assert!(
        files
            .iter()
            .any(|file| { file.path == ".github/workflows/job-image-candidate.yml" })
    );
    assert!(
        actual
            .iter()
            .all(|path| { files.iter().any(|file| file.path == *path) })
    );
    assert!(validate_generated_workflow_calls(&files, &final_policy).is_err());
    validate_generated_workflow_calls_with_overrides(&files, &final_policy, &overrides)
        .expect("explicit producer classes permit hosted product and adoption workflows");
}

#[test]
fn schema_one_has_no_placement_metadata() {
    let mut config = crate::config::load_config_from_text(CONFIG).expect("valid scale-set config");
    config.schema = 1;
    let catalog = ToolCatalog::pinned();
    let policy = workflow_routing_policy(&config, Some(ExecutionMode::ScaleSet))
        .expect("configured routing policy");
    let (files, _, overrides) = extra_files(&config, &catalog, env!("CARGO_PKG_VERSION"), &policy)
        .expect("schema one has no extra workflows");

    assert_eq!(files, Vec::new());
    assert_eq!(overrides.hosted_only_paths().count(), 0);
}

#[test]
fn unrequested_product_families_receive_no_hosted_classification() {
    let mut config = crate::config::load_config_from_text(CONFIG).expect("valid scale-set config");
    config
        .execution
        .as_mut()
        .expect("execution profile")
        .workflows = BTreeSet::from([RoutingWorkflow::ImageRelease]);
    let catalog = ToolCatalog::pinned();
    let policy = workflow_routing_policy(&config, None).expect("configured routing policy");
    let (files, _, overrides) = extra_files(&config, &catalog, env!("CARGO_PKG_VERSION"), &policy)
        .expect("image release workflows render");
    let actual = overrides
        .hosted_only_paths()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

    assert_eq!(
        actual,
        BTreeSet::from([
            ".github/workflows/product-release.yml".to_owned(),
            ".github/workflows/product-release-images.yml".to_owned(),
        ])
    );
    assert!(files.iter().all(|file| {
        !actual.contains(&file.path)
            || Path::new(&file.path)
                .extension()
                .is_some_and(|extension| extension.as_encoded_bytes() == b"yml")
    }));
}
