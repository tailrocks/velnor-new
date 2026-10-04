//! Fixed internal-operation environment channels.

use crate::{
    render::RenderContext,
    steps::{self, INTERNAL_OP_ENV, REQUEST_FILE_ENV},
    yaml::Yaml,
};

/// Env for one internal step: op plus request file, fetch carries auth, and
/// report staging carries only its private operation.
///
/// The fetch op takes no request file; it reads the plan from the
/// run directory and authenticates `gh` with the job token plus the
/// repository slug (fixed literals, never caller input). Token hygiene
/// still gates IR-level `GH_TOKEN` (see `support`); this render-time
/// pair is fixed by construction for fetch and baseline publication. Merge-family
/// steps in the final job additionally carry the finalized `needs`
/// conclusions channel plus the rendered expected inventory, so the
/// merge binds required validators to the committed workflow.
pub(super) fn internal_env(
    op: &str,
    target: &str,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    early: bool,
) -> Yaml {
    if op == steps::FETCH_OPERATION {
        return Yaml::Map(vec![
            ("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")),
            ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
            (INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())),
        ]);
    }
    if op == steps::STAGE_REPORTS_OPERATION {
        return Yaml::Map(vec![(INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned()))]);
    }
    let request = format!("{}/{target}-request.json", ctx.request_dir);
    let mut env = vec![(INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned()))];
    for (key, value) in needs_envs {
        env.push((key.clone(), Yaml::str(value.clone())));
    }
    env.push((REQUEST_FILE_ENV.to_owned(), Yaml::str(request)));
    if (op == steps::PLAN_OPERATION || op == steps::EARLY_PLAN_OPERATION)
        && target == steps::PLAN_OPERATION
    {
        for (key, value) in &ctx.plan_consumer_env {
            env.push((key.clone(), Yaml::str(value.clone())));
        }
    }
    if early && (op == steps::EARLY_PLAN_OPERATION || op == steps::PLAN_OPERATION) {
        env.push(("VELNOR_PLAN_FRESHNESS".to_owned(), Yaml::str("1")));
        if op == steps::PLAN_OPERATION {
            env.push((
                "VELNOR_EARLY_NEEDS_CARGO".to_owned(),
                Yaml::str("${{ steps.early_plan.outputs.needs_cargo }}"),
            ));
        }
    }
    if op == steps::EARLY_PLAN_OPERATION || (early && op == steps::PLAN_OPERATION) {
        env.push(("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")));
        env.push(("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")));
    }
    if op == steps::PUBLISH_OPERATION {
        env.push(("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")));
        env.push(("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")));
    }
    Yaml::Map(env)
}
