use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::{ExecutionMode, PullRequestCachePolicy, ScaleSetSelector};
use velnor_actions_workflow_renderer::WorkflowRoutingPolicy;
use velnor_actions_workflow_renderer::cache_cleanup::CACHE_CLEANUP_PATH;

use super::files_for;

const IMAGE: &str = "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )
    .expect("The test selector is invalid.")
}

#[test]
fn the_workflow_exists_only_where_pull_requests_write_scoped_entries() {
    let hosted = WorkflowRoutingPolicy::hosted_only();
    assert!(
        files_for(
            PullRequestCachePolicy::ReadOnly,
            "ubuntu-26.04",
            &hosted,
            None,
        )
        .expect("read-only")
        .is_empty(),
        "read-only pull requests write nothing to clean"
    );
    let scoped = files_for(
        PullRequestCachePolicy::SameRepositoryScoped,
        "ubuntu-26.04",
        &hosted,
        None,
    )
    .expect("scoped");
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].path, CACHE_CLEANUP_PATH);
}

#[test]
fn scale_set_cleanup_uses_the_selected_selector_and_immutable_container() {
    let image =
        ValidatedImageReference::parse(IMAGE).expect("The test image reference is not immutable.");
    let policy =
        WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), Some(image.clone()));
    let selected = selector();
    let files = files_for(
        PullRequestCachePolicy::SameRepositoryScoped,
        "ubuntu-26.04",
        &policy,
        Some((&selected, Some(&image))),
    )
    .expect("The test could not render Scale Set cleanup.");
    assert_eq!(files.len(), 1);
    assert!(
        files[0]
            .bytes
            .contains("runs-on: [velnor, ubuntu-26.04-scale-set]")
    );
    assert!(files[0].bytes.contains(&format!("image: {IMAGE}")));
    assert!(!files[0].bytes.contains("runs-on: ubuntu-latest"));
}

#[test]
fn scale_set_cleanup_can_remain_containerless() {
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), None);
    let selected = selector();
    let files = files_for(
        PullRequestCachePolicy::SameRepositoryScoped,
        "ubuntu-26.04",
        &policy,
        Some((&selected, None)),
    )
    .expect("The selector-only cleanup job remains containerless.");
    assert_eq!(files.len(), 1);
    assert!(
        files[0]
            .bytes
            .contains("runs-on: [velnor, ubuntu-26.04-scale-set]\n")
    );
    assert!(!files[0].bytes.contains("container:"));
    assert!(!files[0].bytes.contains("ubuntu-latest"));
}

#[test]
fn both_mode_uses_the_existing_hosted_control_job_choice() {
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::Both, selector(), None);
    let files = files_for(
        PullRequestCachePolicy::SameRepositoryScoped,
        "ubuntu-26.04",
        &policy,
        None,
    )
    .expect("The test could not render cleanup for the control job in Both mode.");
    assert!(files[0].bytes.contains("runs-on: ubuntu-26.04\n"));
    assert!(!files[0].bytes.contains("container:"));
}
