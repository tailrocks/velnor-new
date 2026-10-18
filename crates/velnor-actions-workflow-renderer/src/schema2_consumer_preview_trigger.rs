//! Dedicated native preview completion admission; generic triggers stay restricted.
use velnor_actions_contract::package_release::PackageReleaseChannel;

use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::{WORKFLOW_DISPLAY_NAME, package_release::NativePreviewEvent};

pub(in super::super) fn trigger(branch: &str) -> (String, Yaml) {
    (
        NativePreviewEvent::WorkflowRun.as_str().to_owned(),
        Yaml::Map(vec![
            (
                "workflows".to_owned(),
                Yaml::Seq(vec![Yaml::annotated(
                    WORKFLOW_DISPLAY_NAME,
                    "zizmor: ignore[dangerous-triggers]",
                )]),
            ),
            ("types".to_owned(), Yaml::Seq(vec![Yaml::str("completed")])),
            ("branches".to_owned(), Yaml::Seq(vec![Yaml::str(branch)])),
        ]),
    )
}

/// The annotation covers zizmor's whole `on` span, so admit only this closed shape.
/// Unknown/duplicate/sibling triggers must fail before any annotated bytes emit.
pub(in super::super) fn validate(document: &Yaml, recovery: Yaml) -> Result<(), RenderError> {
    let Yaml::Map(fields) = document else {
        return Err(trigger_error());
    };
    let mut triggers = fields.iter().filter(|(key, _)| key == "on");
    let expected = Yaml::Map(vec![
        trigger("main"),
        ("workflow_dispatch".to_owned(), recovery),
    ]);
    match (triggers.next(), triggers.next()) {
        (Some((_, actual)), None) if actual == &expected => Ok(()),
        _ => Err(trigger_error()),
    }
}

fn trigger_error() -> RenderError {
    RenderError::InvalidWorkflow("native_preview_trigger_mismatch".to_owned())
}

pub(super) fn admission(channel: PackageReleaseChannel) -> &'static str {
    match channel {
        PackageReleaseChannel::Preview => ADMISSION,
        PackageReleaseChannel::Stable => "",
    }
}

pub(super) fn assertion(channel: PackageReleaseChannel) -> &'static str {
    match channel {
        PackageReleaseChannel::Preview => "assert_preview_completion \"$run_id\" \"$run_attempt\"",
        PackageReleaseChannel::Stable => "",
    }
}

const ADMISSION: &str = r#"assert_preview_completion() {
  [[ "${GITHUB_EVENT_NAME-}" == 'workflow_run' ]] || return 0
  local expected_run="$1" expected_attempt="$2"
  local repository_id="${GITHUB_REPOSITORY_ID-}"
  [[ "$repository_id" =~ ^[1-9][0-9]*$ ]] || fail 'repository ID is invalid'
  [[ -f "${GITHUB_EVENT_PATH-}" && ! -L "$GITHUB_EVENT_PATH" ]] || fail 'completion event file is missing or linked'
  local repo_info workflow event_run authenticated
  repo_info="$(gh_api "repos/$repository")" || fail 'completion repository lookup failed'
  jq -e --arg repository "$repository" --argjson id "$repository_id" \
    '.id == $id and .full_name == $repository' <<<"$repo_info" >/dev/null \
    || fail 'completion repository identity mismatch'
  workflow="$(gh_api "repos/$repository/actions/workflows/@CI_WORKFLOW_API_ID@")" \
    || fail 'completion workflow lookup failed'
  local workflow_id
  workflow_id="$(jq -er --arg name '@CI_WORKFLOW_NAME@' --arg path '@CI_WORKFLOW_PATH@' \
    'if .name == $name and .path == $path and .state == "active"
        and (.id | type == "number" and . > 0 and . == floor)
     then .id else error("workflow identity") end' <<<"$workflow")" \
    || fail 'completion workflow identity mismatch'
  event_run="$(jq -ec --arg repository "$repository" --argjson repository_id "$repository_id" \
    --argjson workflow_id "$workflow_id" --arg name '@CI_WORKFLOW_NAME@' --arg path '@CI_WORKFLOW_PATH@' \
    'if .action == "completed" and .repository.id == $repository_id
        and .repository.full_name == $repository
        and .workflow.id == $workflow_id and .workflow.name == $name and .workflow.path == $path
     then .workflow_run else error("completion event identity") end' "$GITHUB_EVENT_PATH")" \
    || fail 'completion event is invalid'
  assert_completion_run "$event_run" "$expected_run" "$expected_attempt" "$repository_id" "$workflow_id"
  authenticated="$(gh_api "repos/$repository/actions/runs/$expected_run")" \
    || fail 'completion run lookup failed'
  assert_completion_run "$authenticated" "$expected_run" "$expected_attempt" "$repository_id" "$workflow_id"
}

assert_completion_run() {
  jq -e --arg repository "$repository" --arg source_sha "$source_sha" \
    --arg branch "$default_branch" --arg path '@CI_WORKFLOW_PATH@' --arg name '@CI_WORKFLOW_NAME@' \
    --argjson run_id "$2" --argjson attempt "$3" --argjson repository_id "$4" --argjson workflow_id "$5" \
    '.id == $run_id and .run_attempt == $attempt and .workflow_id == $workflow_id
     and .path == $path and .name == $name and .event == "push"
     and .head_sha == $source_sha and .head_branch == $branch
     and .status == "completed" and .conclusion == "success"
     and .repository.id == $repository_id and .repository.full_name == $repository
     and .head_repository.id == $repository_id and .head_repository.full_name == $repository' \
    <<<"$1" >/dev/null || fail 'completion run identity mismatch'
}"#;
