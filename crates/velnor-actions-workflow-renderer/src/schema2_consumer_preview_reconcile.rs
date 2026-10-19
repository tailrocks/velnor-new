//! Fixed retained-release tap continuation; no publication or producer route.
use super::{
    ConsumerPreviewSpec, RenderError, RenderedFile, TapInvocationContext, Yaml, acquire, base,
    finish, run_step, steps, tap,
};
use crate::commands::join_argv_for_run;
use velnor_actions_contract::{JobTimeout, package_release::PackageReleaseChannel};
/// Fixed default-branch continuation workflow filename.
pub(super) const PREVIEW_TAP_RECONCILE_PATH: &str = ".github/workflows/preview-tap-reconcile.yml";
/// Render only read-only selection and existing independently gated tap stages.
/// # Errors
/// Rejects missing promotion policy, malformed inventory or unsupported pins.
pub fn render_preview_tap_reconcile(
    spec: &ConsumerPreviewSpec,
    routing: &crate::workflow_routing::WorkflowRoutingPolicy,
) -> Result<RenderedFile, RenderError> {
    spec.plan
        .validate()
        .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?;
    let tap = spec
        .homebrew_tap
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("tap_reconcile_policy_missing".to_owned()))?;
    if spec.plan.channel != PackageReleaseChannel::Preview || spec.plan.default_branch != "main" {
        return Err(RenderError::InvalidWorkflow(
            "tap_reconcile_plan_invalid".to_owned(),
        ));
    }
    let mut jobs = vec![selection(spec)?];
    jobs.extend(tap::jobs(spec, tap, TapInvocationContext::Reconcile)?);
    let document = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Preview tap reconciliation")),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                (
                    "schedule".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![(
                        "cron".to_owned(),
                        Yaml::str("*/5 * * * *"),
                    )])]),
                ),
                ("workflow_dispatch".to_owned(), Yaml::Map(Vec::new())),
            ]),
        ),
        ("permissions".to_owned(), Yaml::Map(Vec::new())),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("native-preview-tap-reconcile-${{ github.repository }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]);
    let document = crate::workflow_routing::route_workflow_yaml(
        PREVIEW_TAP_RECONCILE_PATH,
        document,
        routing,
    )?;
    Ok(RenderedFile {
        path: PREVIEW_TAP_RECONCILE_PATH.to_owned(),
        bytes: crate::render::fallback::render_checked_workflow(
            PREVIEW_TAP_RECONCILE_PATH,
            &document,
            &spec.generator_version,
        )?,
    })
}
fn selection(spec: &ConsumerPreviewSpec) -> Result<(String, Yaml), RenderError> {
    let id = "preview-tap-selection";
    let mut fields = base(
        "Select the current immutable preview",
        Yaml::str("ubuntu-26.04"),
        JobTimeout::DEFAULT,
    );
    fields.push((
        "permissions".to_owned(),
        steps::permissions(&[
            ("contents", "read"),
            ("actions", "read"),
            ("attestations", "read"),
        ]),
    ));
    let outputs = [
        "eligible",
        "source_sha",
        "original_run_id",
        "original_run_attempt",
        "ci_run_id",
        "ci_attempt",
        "commit_count",
    ];
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(
            outputs
                .into_iter()
                .map(|key| {
                    (
                        key.to_owned(),
                        Yaml::str(format!("${{{{ steps.selection.outputs.{key} }}}}")),
                    )
                })
                .collect(),
        ),
    ));
    Ok(finish(
        id,
        fields,
        vec![
            steps::history_checkout(),
            steps::setup(&spec.linux_setup),
            run_step(
                "Install pinned GitHub CLI",
                &join_argv_for_run(&spec.install_gh_argv)?,
            ),
            acquire(&spec.linux_acquire, id, "ubuntu-26.04")?,
            steps::history_count(TapInvocationContext::Reconcile),
            selector_operation(spec)?,
        ],
    ))
}
fn selector_operation(spec: &ConsumerPreviewSpec) -> Result<Yaml, RenderError> {
    let plan = String::from_utf8(
        spec.plan
            .canonical_bytes()
            .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?,
    )
    .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?;
    let run = format!(
        "set -euo pipefail\numask 077\nmkdir -p \"$RUNNER_TEMP/velnor\"\n(set -C; printf '%s' \"$VELNOR_RELEASE_PLAN\" > \"$VELNOR_REQUEST_FILE\")\n\"$RUNNER_TEMP/velnor/bin/velnor-actions-{}\"\n",
        spec.generator_version
    );
    Ok(steps::script(
        "Authenticate current main and retained immutable preview",
        "selection",
        &run,
        &[
            ("VELNOR_INTERNAL_OP", "select-preview-tap-continuation-v1"),
            ("VELNOR_RELEASE_PLAN", &plan),
            (
                "VELNOR_REQUEST_FILE",
                "${{ runner.temp }}/velnor/package-release-plan.json",
            ),
            (
                "VELNOR_RELEASE_COMMIT_COUNT",
                "${{ steps.history.outputs.commit_count }}",
            ),
            ("GH_TOKEN", "${{ github.token }}"),
        ],
    ))
}
