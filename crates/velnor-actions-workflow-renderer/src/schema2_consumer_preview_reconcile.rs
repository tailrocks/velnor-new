//! Fixed retained-release tap continuation; no publication or producer route.
use super::{
    ConsumerReleaseSpec, RenderError, RenderedFile, TapInvocationContext, Yaml, acquire, base,
    finish, run_step, steps, tap,
};
use crate::commands::join_argv_for_run;
use velnor_actions_contract::{JobTimeout, package_release::PackageReleaseChannel};
fn context(spec: &ConsumerReleaseSpec) -> TapInvocationContext {
    match spec.policy.channel {
        PackageReleaseChannel::Preview => TapInvocationContext::Reconcile,
        PackageReleaseChannel::Stable => TapInvocationContext::StableReconcile,
    }
}
/// Render only read-only selection and existing independently gated tap stages.
/// # Errors
/// Rejects missing promotion policy, malformed inventory or unsupported pins.
pub fn render_release_tap_reconcile(
    spec: &ConsumerReleaseSpec,
) -> Result<RenderedFile, RenderError> {
    spec.policy
        .validate()
        .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?;
    let tap = spec
        .homebrew_tap
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("tap_reconcile_policy_missing".to_owned()))?;
    if spec.policy.default_branch != "main" {
        return Err(RenderError::InvalidWorkflow(
            "tap_reconcile_plan_invalid".to_owned(),
        ));
    }
    let mut jobs = vec![selection(spec)?];
    jobs.extend(tap::jobs(spec, tap, context(spec))?);
    let document = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(spec.policy.channel.tap_reconcile_workflow_name()),
        ),
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
                    Yaml::str(format!(
                        "native-{}-tap-reconcile-${{{{ github.repository }}}}",
                        context(spec).prefix()
                    )),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]);
    Ok(RenderedFile {
        path: spec.policy.channel.tap_reconcile_workflow_path().to_owned(),
        bytes: crate::render::fallback::render_checked_workflow(
            spec.policy.channel.tap_reconcile_workflow_path(),
            &document,
            &spec.generator_version,
        )?,
    })
}
fn selection(spec: &ConsumerReleaseSpec) -> Result<(String, Yaml), RenderError> {
    let id = context(spec).selection();
    let mut fields = base(
        match spec.policy.channel {
            PackageReleaseChannel::Preview => "Select the current committed immutable preview",
            PackageReleaseChannel::Stable => "Select the authenticated permanent stable release",
        },
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
            steps::history_checkout(spec),
            steps::setup(&spec.linux_setup),
            run_step(
                "Install pinned GitHub CLI",
                &join_argv_for_run(&spec.install_gh_argv)?,
            ),
            acquire(&spec.linux_acquire, id, "ubuntu-26.04")?,
            steps::history_count(context(spec)),
            selector_operation(spec)?,
        ],
    ))
}
fn selector_operation(spec: &ConsumerReleaseSpec) -> Result<Yaml, RenderError> {
    let plan = String::from_utf8(
        spec.policy
            .canonical_bytes()
            .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?,
    )
    .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?;
    let run = format!(
        "set -euo pipefail\numask 077\nmkdir -p \"$RUNNER_TEMP/velnor\"\n(set -C; printf '%s' \"$VELNOR_RELEASE_POLICY\" > \"$VELNOR_REQUEST_FILE\")\n\"$RUNNER_TEMP/velnor/bin/velnor-actions-{}\"\n",
        spec.generator_version
    );
    let mut env = vec![
        ("VELNOR_INTERNAL_OP", "select-release-tap-continuation-v1"),
        ("VELNOR_RELEASE_POLICY", plan.as_str()),
        (
            "VELNOR_REQUEST_FILE",
            "${{ runner.temp }}/velnor/package-release-policy.json",
        ),
        (
            "VELNOR_RELEASE_COMMIT_COUNT",
            "${{ steps.history.outputs.commit_count }}",
        ),
        ("GH_TOKEN", "${{ github.token }}"),
        (
            "VELNOR_RELEASE_STATUS_TOKEN",
            "${{ secrets.RELEASE_STATUS_READ_TOKEN }}",
        ),
    ];
    if let Some(stable) = &spec.stable {
        env.push((
            "VELNOR_STABLE_VERSION_PACKAGE",
            stable.version_package.as_str(),
        ));
    }
    Ok(steps::script(
        "Authenticate the committed signed channel generation",
        "selection",
        &run,
        &env,
    ))
}
