//! Cache seed step builders: checkout, skeleton, lanes, trim, janitor.
//!
//! Every step is fixed generated shell; the helper binary only returns
//! data (`resolve-seed-task-v1` frames, `seed-skeleton-v1` stubs,
//! `seed-trim-v1` deletions and outputs). Lanes run one crate at a time,
//! record a failing crate, continue, and fail the step at its end.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, SEED_CHECKOUT_REF, SeedLane, trusted_root_prefix,
};
use velnor_actions_contract::{Step, StepId, StepRole};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::steps::{
    INTERNAL_OP_ENV, STAGED_BINARY_PREFIX, action_step, ambient_shell_step, shell_step,
};

use crate::OrchestratorError;

/// Shell spelling of the seed scratch directory.
const SEED_DIR: &str = "$RUNNER_TEMP/velnor/seed";
/// Data-only op resolving one lane record into a NUL frame.
pub(crate) const RESOLVE_SEED_TASK_OP: &str = "resolve-seed-task-v1";
/// Data-only op stubbing the root's workspace member sources.
pub(crate) const SEED_SKELETON_OP: &str = "seed-skeleton-v1";
/// Data-only op trimming the lane profiles to dependency units.
pub(crate) const SEED_TRIM_OP: &str = "seed-trim-v1";

fn render(err: velnor_actions_workflow_renderer::RenderError) -> OrchestratorError {
    OrchestratorError::from(err)
}

fn sh(
    name: &str,
    script: &[String],
    env: BTreeMap<String, String>,
) -> Result<Step, OrchestratorError> {
    shell_step(
        name,
        vec!["sh".to_owned(), "-c".to_owned(), script.join("; ")],
        env,
    )
    .map_err(render)
}

/// Root spelling for helper env (`.` is the repository root).
pub(crate) fn root_env(root: &str) -> String {
    if root.is_empty() {
        ".".to_owned()
    } else {
        root.to_owned()
    }
}

/// Credential-free checkout of the triggering commit at the crate jobs' path.
/// # Errors
pub(crate) fn checkout_step() -> Result<Step, OrchestratorError> {
    let with = BTreeMap::from([
        ("fetch-depth".to_owned(), "1".to_owned()),
        ("persist-credentials".to_owned(), "false".to_owned()),
        ("ref".to_owned(), SEED_CHECKOUT_REF.to_owned()),
    ]);
    let mut step = action_step("Checkout", crate::workflow::CHECKOUT_USES, with).map_err(render)?;
    step.role = Some(StepRole::Checkout);
    Ok(step)
}

/// Read the root's resolve graph, then stub its member sources in place.
/// # Errors
pub(crate) fn skeleton_steps(
    catalog: &ToolCatalog,
    root: &str,
    helper_version: &str,
) -> Result<[Step; 2], OrchestratorError> {
    let homes = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    let manifest = crate::discover::workspace_manifest(root);
    let spec = catalog.tool_spec(PinnedTool::Rust);
    let mut metadata = sh(
        "Read seed metadata",
        &[
            "set -eu".to_owned(),
            format!("mkdir -p \"{SEED_DIR}\" \"$RUNNER_TEMP/velnor/cargo-clean\""),
            "cd \"$RUNNER_TEMP/velnor/cargo-clean\"".to_owned(),
            format!(
                "mise --no-config --no-env --no-hooks exec {spec} -- cargo metadata --format-version 1 --locked --offline --all-features --manifest-path \"$GITHUB_WORKSPACE/{manifest}\" > \"{SEED_DIR}/metadata.json\""
            ),
        ],
        homes,
    )?;
    metadata.role = Some(StepRole::SeedSkeleton);
    let mut stub = shell_step(
        "Stub workspace members",
        vec![format!("{STAGED_BINARY_PREFIX}{helper_version}")],
        BTreeMap::from([
            (INTERNAL_OP_ENV.to_owned(), SEED_SKELETON_OP.to_owned()),
            ("VELNOR_SEED_ROOT".to_owned(), root_env(root)),
        ]),
    )
    .map_err(render)?;
    stub.role = Some(StepRole::SeedSkeleton);
    Ok([metadata, stub])
}

/// One per-root lane: replay every selected record, one crate at a time.
/// # Errors
pub(crate) fn lane_step(
    lane: SeedLane,
    digests: &[String],
    versions: (&str, &str),
    compiler_jobs: u32,
) -> Result<Step, OrchestratorError> {
    let (helper_version, generator_version) = versions;
    let name = lane.as_str();
    let out = format!("{SEED_DIR}/{name}");
    let mut script = lane_prelude(&out);
    script.push(format!("h=\"{STAGED_BINARY_PREFIX}{helper_version}\""));
    if !digests.is_empty() {
        script.push(format!(
            "for digest in {}; do if {INTERNAL_OP_ENV}={RESOLVE_SEED_TASK_OP} VELNOR_TASK_EXECUTION_DIGEST=\"$digest\" VELNOR_SEED_LANE={name} VELNOR_GENERATOR_VERSION={generator_version} \"$h\" > \"$o/$digest.frame\"; then if xargs -0 -a \"$o/$digest.frame\" env -- > \"$o/$digest.jsonl\"; then :; else failed=1; printf 'velnor: seed {name} lane failed: %s\\n' \"$digest\" >&2; fi; else failed=1; printf 'velnor: seed {name} record rejected: %s\\n' \"$digest\" >&2; fi; done",
            digests.join(" ")
        ));
    }
    script.extend(lane_epilogue());
    lane_from(lane, &script, BTreeMap::new(), compiler_jobs)
}

/// The bootstrap helper lane: the exact Plan helper build with JSON output.
/// # Errors
pub(crate) fn helper_lane_step(
    catalog: &ToolCatalog,
    build: &[String],
    compiler_jobs: u32,
) -> Result<Step, OrchestratorError> {
    let at = build
        .iter()
        .position(|arg| arg == "build")
        .ok_or_else(|| contract("helper_build_without_subcommand"))?;
    let mut argv = build.to_vec();
    let insert = at + 1;
    argv.splice(
        insert..insert,
        [
            "--message-format".to_owned(),
            "json-render-diagnostics".to_owned(),
        ],
    );
    let out = format!("{SEED_DIR}/{}", SeedLane::Helper.as_str());
    let mut script = lane_prelude(&out);
    script.push(format!(
        "if {} > \"$o/helper.jsonl\"; then :; else failed=1; printf '%s\\n' 'velnor: seed helper lane failed' >&2; fi",
        argv.join(" ")
    ));
    script.extend(lane_epilogue());
    let homes = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    lane_from(SeedLane::Helper, &script, homes, compiler_jobs)
}

fn lane_prelude(out: &str) -> Vec<String> {
    vec![
        "set -eu".to_owned(),
        format!("o=\"{out}\""),
        "rm -rf \"$o\"".to_owned(),
        "mkdir -p \"$o\"".to_owned(),
        "failed=0".to_owned(),
        "s=$(date +%s%3N)".to_owned(),
    ]
}

fn lane_epilogue() -> Vec<String> {
    vec![
        "e=$(date +%s%3N)".to_owned(),
        "printf '%s %s\\n' \"$s\" \"$e\" > \"$o/timing\"".to_owned(),
        "[ \"$failed\" = 0 ]".to_owned(),
    ]
}

fn lane_from(
    lane: SeedLane,
    script: &[String],
    mut env: BTreeMap<String, String>,
    compiler_jobs: u32,
) -> Result<Step, OrchestratorError> {
    env.insert("CARGO_BUILD_JOBS".to_owned(), compiler_jobs.to_string());
    let title = match lane {
        SeedLane::Build => "Seed build lane",
        SeedLane::Clippy => "Seed clippy lane",
        SeedLane::Doc => "Seed doc lane",
        SeedLane::Helper => "Seed helper lane",
    };
    let mut step = sh(title, script, env)?;
    step.role = Some(StepRole::SeedLane);
    step.id = Some(match lane {
        SeedLane::Build => StepId::SeedBuild,
        SeedLane::Clippy => StepId::SeedClippy,
        SeedLane::Doc => StepId::SeedDoc,
        SeedLane::Helper => StepId::SeedHelper,
    });
    Ok(step)
}

/// Trim every lane profile to registry and Git dependency units.
/// # Errors
pub(crate) fn trim_step(
    root: &str,
    lanes: &[SeedLane],
    helper_version: &str,
    ceiling: u64,
    keys: (&str, Option<&str>),
) -> Result<Step, OrchestratorError> {
    let lane_list = lanes
        .iter()
        .map(|lane| lane.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let mut env = BTreeMap::from([
        (INTERNAL_OP_ENV.to_owned(), SEED_TRIM_OP.to_owned()),
        ("VELNOR_SEED_ROOT".to_owned(), root_env(root)),
        ("VELNOR_SEED_LANES".to_owned(), lane_list),
        ("VELNOR_SEED_MAX_BYTES".to_owned(), ceiling.to_string()),
        ("VELNOR_SEED_KEY".to_owned(), keys.0.to_owned()),
    ]);
    if let Some(helper) = keys.1 {
        env.insert("VELNOR_SEED_HELPER_KEY".to_owned(), helper.to_owned());
    }
    let mut step = shell_step(
        "Trim dependency seed",
        vec![format!("{STAGED_BINARY_PREFIX}{helper_version}")],
        env,
    )
    .map_err(render)?;
    step.id = Some(StepId::SeedTrim);
    step.role = Some(StepRole::SeedTrim);
    Ok(step)
}

/// Delete every other generation of one family and root on the default branch.
/// # Errors
pub(crate) fn janitor_step(
    catalog: &ToolCatalog,
    family: CacheFamily,
    root: &str,
    default_branch: &str,
    key: &str,
    save: StepId,
) -> Result<Step, OrchestratorError> {
    let gh = catalog.tool_spec(PinnedTool::Gh);
    let prefix = trusted_root_prefix(family, root);
    let exec = format!("mise --no-config --no-env --no-hooks exec {gh} -- gh");
    let list = format!("{SEED_DIR}/janitor-{}.tsv", family.spec().layer);
    let script = [
        "set -eu".to_owned(),
        format!("mkdir -p \"{SEED_DIR}\""),
        "keep=\"$VELNOR_SEED_KEY\"".to_owned(),
        format!("mise --no-config --no-env --no-hooks install {gh}"),
        format!(
            "{exec} cache list --repo \"$GITHUB_REPOSITORY\" --ref 'refs/heads/{default_branch}' --key '{prefix}' --limit 100 --json id,key --jq '.[] | [.id, .key] | @tsv' > \"{list}\""
        ),
        format!(
            "while IFS='\t' read -r id entry; do if [ \"$entry\" != \"$keep\" ]; then {exec} cache delete --repo \"$GITHUB_REPOSITORY\" \"$id\"; printf 'velnor: seed janitor deleted %s\\n' \"$entry\"; fi; done < \"{list}\""
        ),
    ];
    let name = if family == CacheFamily::RsdepsHelper {
        "Delete superseded helper dependency seeds"
    } else {
        "Delete superseded dependency seeds"
    };
    let mut step = ambient_shell_step(
        name,
        vec!["sh".to_owned(), "-c".to_owned(), script.join("; ")],
        // The renderer binds `GH_TOKEN` to this role at render time.
        BTreeMap::from([("VELNOR_SEED_KEY".to_owned(), key.to_owned())]),
    )
    .map_err(render)?;
    step.role = Some(StepRole::SeedJanitor);
    // A later failed lane (the helper's) must not leave a superseded
    // generation behind once this family's own save succeeded.
    step.condition = Some(format!(
        "always() && !cancelled() && steps.{}.outcome == 'success'",
        save.as_str()
    ));
    Ok(step)
}

fn contract(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}
