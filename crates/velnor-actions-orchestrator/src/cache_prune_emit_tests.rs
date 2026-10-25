use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::{ExecutionMode, ScaleSetSelector};
use velnor_actions_workflow_renderer::WorkflowRoutingPolicy;
use velnor_actions_workflow_renderer::cache_prune::CACHE_PRUNE_PATH;

use super::files_for;

const IMAGE: &str = "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )
    .expect("The test selector is valid.")
}

#[test]
fn the_workflow_exists_only_where_a_cache_family_can_be_written() {
    let hosted = WorkflowRoutingPolicy::hosted_only();
    assert_eq!(
        files_for(false, "ubuntu-26.04", "main", &hosted, None)
            .expect("Generation without cache writers must succeed.")
            .len(),
        0
    );
    let files = files_for(true, "ubuntu-26.04", "trunk", &hosted, None)
        .expect("The hosted workflow renders.");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, CACHE_PRUNE_PATH);
    assert!(files[0].bytes.contains("      - trunk\n"));
}

#[test]
fn scale_set_pruning_uses_the_selected_container_without_hosted_fallback() {
    let image = ValidatedImageReference::parse(IMAGE).expect("The test image is immutable.");
    let policy =
        WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), Some(image.clone()));
    let selected = selector();
    let files = files_for(
        true,
        "ubuntu-26.04",
        "main",
        &policy,
        Some((&selected, Some(&image))),
    )
    .expect("The Scale Set workflow renders.");
    assert!(
        files[0]
            .bytes
            .contains("runs-on: [velnor, ubuntu-26.04-scale-set]\n")
    );
    assert!(files[0].bytes.contains(&format!("image: {IMAGE}\n")));
    assert!(!files[0].bytes.contains("ubuntu-latest"));
    assert!(!files[0].bytes.contains("gh cache"));
}

#[test]
fn selector_only_pruning_remains_containerless() {
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), None);
    let selected = selector();
    let files = files_for(
        true,
        "ubuntu-26.04",
        "main",
        &policy,
        Some((&selected, None)),
    )
    .expect("The selector-only pruning job can use its host runtime.");
    assert_eq!(files.len(), 1);
    assert!(
        files[0]
            .bytes
            .contains("runs-on: [velnor, ubuntu-26.04-scale-set]\n")
    );
    assert!(!files[0].bytes.contains("container:"));
}
