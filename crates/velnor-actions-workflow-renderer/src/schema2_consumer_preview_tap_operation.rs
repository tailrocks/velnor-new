//! Credential placement for the two closed tap invocation routes.
use super::{
    ConsumerReleaseSpec, ConsumerReleaseTapSpec, RenderError, TapInvocationContext, Yaml, steps,
    variable,
};
use velnor_actions_contract::canonical_json_bytes;
pub(super) fn operation(
    spec: &ConsumerReleaseSpec,
    tap: &ConsumerReleaseTapSpec,
    op: &str,
    merge: bool,
    context: TapInvocationContext,
) -> Result<Yaml, RenderError> {
    let mut step = steps::operation(spec, op, op != "prepare-homebrew-tap-v1", context)?;
    let policy = String::from_utf8(
        canonical_json_bytes(&tap.policy)
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?,
    )
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let mut values = vec![("VELNOR_TAP_POLICY".to_owned(), Yaml::str(policy))];
    if matches!(op, "write-homebrew-tap-pr-v1" | "merge-homebrew-tap-pr-v1") {
        values.extend(writer_values(tap, merge));
    }
    if op == "prepare-homebrew-tap-v1" {
        values.push((
            "VELNOR_TAP_ADMISSION_DIGEST".to_owned(),
            Yaml::str("${{ steps.tap-admission.outputs.admission_digest }}"),
        ));
    }
    if op == "admit-homebrew-tap-v1"
        && let Yaml::Map(fields) = &mut step
        && let Some((_, id)) = fields.iter_mut().find(|(key, _)| key == "id")
    {
        *id = Yaml::str("tap-admission");
    }
    if merge {
        values.extend(observer_values(tap));
    }
    if let Yaml::Map(fields) = &mut step
        && let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(key, _)| key == "env")
    {
        env.extend(values);
    }
    Ok(step)
}

fn writer_values(tap: &ConsumerReleaseTapSpec, merge: bool) -> Vec<(String, Yaml)> {
    vec![
        (
            "VELNOR_TAP_APP_ID".to_owned(),
            variable(&tap.policy.app_id_variable),
        ),
        (
            "VELNOR_TAP_APP_SLUG".to_owned(),
            Yaml::str("${{ steps.tap-token.outputs.app-slug }}"),
        ),
        (
            "VELNOR_TAP_INSTALLATION_ID".to_owned(),
            Yaml::str("${{ steps.tap-token.outputs.installation-id }}"),
        ),
        (
            (if merge {
                "VELNOR_TAP_MERGE_TOKEN"
            } else {
                "VELNOR_TAP_TOKEN"
            })
            .to_owned(),
            Yaml::str("${{ steps.tap-token.outputs.token }}"),
        ),
    ]
}

fn observer_values(tap: &ConsumerReleaseTapSpec) -> Vec<(String, Yaml)> {
    let mut values = vec![
        (
            "VELNOR_TAP_PROTECTION_TOKEN".to_owned(),
            Yaml::str("${{ steps.tap-protection-token.outputs.token }}"),
        ),
        (
            "VELNOR_TAP_PROTECTION_APP_ID".to_owned(),
            variable(&tap.policy.protection_observer.app_id_variable),
        ),
        (
            "VELNOR_TAP_PROTECTION_APP_SLUG".to_owned(),
            Yaml::str("${{ steps.tap-protection-token.outputs.app-slug }}"),
        ),
        (
            "VELNOR_TAP_PROTECTION_INSTALLATION_ID".to_owned(),
            Yaml::str("${{ steps.tap-protection-token.outputs.installation-id }}"),
        ),
    ];
    values.push((
        "VELNOR_TAP_COMPLETION_APP_ID".to_owned(),
        variable(&tap.policy.review_completion.app_id_variable),
    ));
    values.extend(
        tap.policy
            .review_completion
            .expected_reviewers
            .iter()
            .enumerate()
            .map(|(index, reviewer)| {
                (
                    format!("VELNOR_TAP_REVIEWER_APP_ID_{index}"),
                    variable(&reviewer.app_id_variable),
                )
            }),
    );
    values
}
