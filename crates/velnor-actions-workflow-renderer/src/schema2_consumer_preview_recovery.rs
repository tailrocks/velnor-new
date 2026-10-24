use super::{
    ConsumerReleaseSpec, RenderError, TapInvocationContext, Yaml, acquire, base, finish, needs,
    operation, permissions, publication_budget, run_step, setup,
};
use crate::commands::join_argv_for_run;
use velnor_actions_contract::package_release::PackageReleaseChannel;

pub(in crate::schema2) fn recovery(
    spec: &ConsumerReleaseSpec,
) -> Result<(String, Yaml), RenderError> {
    let context = match spec.policy.channel {
        PackageReleaseChannel::Preview => TapInvocationContext::Preview,
        PackageReleaseChannel::Stable => TapInvocationContext::StableAuthority,
    };
    let id = format!("{}-recovery", context.prefix());
    let mut fields = base(
        "Recover the original signed release without rebuilding",
        Yaml::str("ubuntu-26.04"),
        spec.publication_timeout_minutes,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str(match spec.policy.channel { PackageReleaseChannel::Preview => "${{ github.event_name == 'workflow_dispatch' }}", PackageReleaseChannel::Stable => "${{ github.event_name == 'workflow_dispatch' && inputs.operation == 'release' && inputs.original_run_id != '' && inputs.original_run_attempt != '' }}" }),
    ));
    fields.push((
        "needs".to_owned(),
        needs(&[&format!("{}-eligibility", context.prefix())]),
    ));
    fields.push(("environment".to_owned(), Yaml::str(context.environment())));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("attestations", "read"),
            ("contents", "write"),
        ]),
    ));
    let mut job_steps = vec![
        setup(&spec.linux_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&spec.linux_acquire, &id, "ubuntu-26.04")?,
        operation(spec, "recover-package-release-v1", true, context)?,
    ];
    let preparation_step_count = job_steps.len() - 1;
    publication_budget::apply_preparation_budget(
        &mut fields,
        &mut job_steps[..preparation_step_count],
        spec.publication_timeout_minutes,
    )?;
    Ok(finish(&id, fields, job_steps))
}
