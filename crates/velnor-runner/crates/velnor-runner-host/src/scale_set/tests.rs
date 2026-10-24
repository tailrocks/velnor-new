use super::{
    EnsureError, ScaleSetLookupAction, create_labels, default_group_id, lookup_action,
    selector_valid,
};
use crate::config::ScaleSetSelector;
use crate::journal::ScaleSetCreateState;
use velnor_runner_github::RunnerGroup;

fn group(id: i64, is_default: bool) -> RunnerGroup {
    RunnerGroup {
        id,
        name: format!("group-{id}"),
        is_default,
    }
}

fn selector() -> Result<ScaleSetSelector, String> {
    ScaleSetSelector::try_new(
        "sea-integration-ubuntu-26.04-scale-set",
        &[
            "velnor".to_owned(),
            "sea-integration-ubuntu-26.04-scale-set".to_owned(),
            "sea-integration".to_owned(),
        ],
    )
    .map_err(|error| error.to_string())
}

#[test]
fn default_group_uses_the_repository_service_result() {
    let groups = [group(1, false), group(7, true)];
    assert_eq!(default_group_id(&groups), Ok(7));
}

#[test]
fn default_group_requires_one_positive_default() {
    assert_eq!(default_group_id(&[]), Err(EnsureError::Rejected));
    assert_eq!(
        default_group_id(&[group(1, false)]),
        Err(EnsureError::Rejected)
    );
    assert_eq!(
        default_group_id(&[group(2, true), group(3, true)]),
        Err(EnsureError::Rejected)
    );
    assert_eq!(
        default_group_id(&[group(0, true)]),
        Err(EnsureError::Rejected)
    );
}

#[test]
fn configured_selector_requires_canonical_labels() -> Result<(), String> {
    let selector = selector()?;
    assert!(selector_valid(&selector));
    let labels = create_labels(selector.labels());
    assert_eq!(
        labels
            .iter()
            .map(|label| label.name.as_str())
            .collect::<Vec<_>>(),
        [
            "velnor",
            "sea-integration-ubuntu-26.04-scale-set",
            "sea-integration",
        ]
    );
    assert!(labels.iter().all(|label| label.label_type == "System"));
    let invalid = ScaleSetSelector::try_new(
        "sea-integration-ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04".to_owned()],
    );
    assert!(invalid.is_err());
    Ok(())
}

#[test]
fn uncertain_scale_set_create_never_adopts_by_name_or_retries() {
    for state in [
        ScaleSetCreateState::Requested,
        ScaleSetCreateState::Uncertain,
    ] {
        assert_eq!(lookup_action(state, true), ScaleSetLookupAction::Hold);
        assert_eq!(lookup_action(state, false), ScaleSetLookupAction::Hold);
    }
    assert_eq!(
        lookup_action(ScaleSetCreateState::NoEffect, false),
        ScaleSetLookupAction::Hold
    );
    assert_eq!(
        lookup_action(ScaleSetCreateState::Prepared, false),
        ScaleSetLookupAction::Create
    );
    assert_eq!(
        lookup_action(ScaleSetCreateState::Prepared, true),
        ScaleSetLookupAction::Adopt
    );
}
