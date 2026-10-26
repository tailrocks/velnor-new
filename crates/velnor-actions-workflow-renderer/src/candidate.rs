use std::{collections::BTreeMap, ffi::OsString};

use velnor_actions_contract::{Job, JobTimeout, Step, StepRole};
use velnor_actions_mise::{PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes};

use crate::{
    RenderError,
    artifact_paths::{CANDIDATE_OUTPUT_DIR_EXPR, CANDIDATE_STAGE_DIR_EXPR},
    closure::{FRESHNESS_OUTDIR, download_plan_step, freshness_step_with_claude_mode},
    render::{CandidateSpec, PLAN_JOB_ID, RenderContext},
    steps,
};

pub const VERIFY_MANIFEST_NAME: &str = "Verify candidate manifest";

pub fn candidate_artifact_name(target: &str) -> Result<String, RenderError> {
    let key = velnor_actions_contract::target_key(target).map_err(RenderError::Contract)?;
    Ok(format!("velnor-candidate-{}-{key}", steps::RUN_KEY_EXPR))
}

#[must_use]
pub fn candidate_manifest_verify_script(target: &str) -> String {
    let dir = steps::CANDIDATE_STAGE_DIR;
    let file = steps::CANDIDATE_MANIFEST_FILE;
    format!(
        "line=; rest=; m=\"{dir}/{file}\" && b=\"{dir}/velnor-actions\" && test -f \"$m\" && test -x \"$b\" && read line rest < \"$m\" || [ -n \"$line\" ] && v=${{line#*\\\"schema\\\":}} && v=${{v%%,*}} && [ \"$v\" = 1 ] && c=${{line#*\\\"commit\\\":\\\"}} && c=${{c%%\\\"*}} && [ \"${{#c}}\" = 40 ] && [ \"$c\" = \"$GITHUB_SHA\" ] && t=${{line#*\\\"target\\\":\\\"}} && t=${{t%%\\\"*}} && [ \"$t\" = \"{target}\" ] && tc=${{line#*\\\"toolchain\\\":\\\"}} && tc=${{tc%%\\\"*}} && [ -n \"$tc\" ] && s=${{line#*\\\"sha256\\\":\\\"}} && s=${{s%%\\\"*}} && [ \"${{#s}}\" = 64 ] && sha256sum \"$b\" > \"{dir}/got.txt\" && read got rest < \"{dir}/got.txt\" && [ \"$got\" = \"$s\" ]"
    )
}

pub fn candidate_manifest_verify_step(target: &str) -> Result<Step, RenderError> {
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "candidate_unsupported_target:{target}"
        )));
    }
    steps::shell_step(
        VERIFY_MANIFEST_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            candidate_manifest_verify_script(target),
        ],
        BTreeMap::new(),
    )
}

pub(crate) fn candidate_job(ctx: &RenderContext, spec: &CandidateSpec) -> Result<Job, RenderError> {
    let target = velnor_actions_contract::ReleaseTarget::for_runner_label(&ctx.runs_on)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unsupported_target_for_runner:{}", ctx.runs_on))
        })?;
    let toolchain = toolchain_identity(&spec.build)?;
    let manifest = steps::candidate_manifest_script(target.triple(), &toolchain);
    let artifact = candidate_artifact_name(target.triple())?;
    let candidate_binary = format!("{}/velnor-actions", steps::CANDIDATE_STAGE_DIR);
    Ok(Job {
        services: Vec::new(),
        display_name: "Candidate".to_owned(),
        runs_on: ctx.runs_on.clone(),
        check_runner: None,
        timeout_minutes: JobTimeout::CANDIDATE,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: vec![
            steps::checkout_step(&ctx.checkout_uses)?,
            verifier_step(
                "Verify restored Rust toolchain",
                velnor_actions_contract::workflow::RUST_VERIFIER_USES[0],
                StepRole::VerifyRustManifest,
            )?,
            prepare_candidate_tools_step()?,
            verifier_step(
                "Verify jactionlint",
                velnor_actions_contract::workflow::JACTIONLINT_VERIFIER_USES,
                StepRole::VerifyJactionlint,
            )?,
            verifier_step(
                crate::VERIFY_RUST_TOOLCHAIN_STEP,
                velnor_actions_contract::workflow::RUST_VERIFIER_USES[1],
                StepRole::VerifyRustManifest,
            )?,
            steps::shell_step(
                "Build candidate",
                spec.build.clone(),
                owned_tool_env(&ToolHomes::runner_temp(), &ToolCatalog::pinned())?,
            )?,
            download_plan_step()?,
            steps::shell_step(
                "Write candidate manifest",
                vec!["sh".to_owned(), "-c".to_owned(), manifest],
                BTreeMap::new(),
            )?,
            steps::shell_step(
                "Write candidate attestation",
                vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    steps::candidate_attestation_script(),
                ],
                BTreeMap::new(),
            )?,
            steps::upload_artifact_step(&artifact, CANDIDATE_OUTPUT_DIR_EXPR)?,
            steps::download_artifact_step(&artifact, CANDIDATE_STAGE_DIR_EXPR)?,
            candidate_manifest_verify_step(target.triple())?,
            freshness_step_with_claude_mode(
                &candidate_binary,
                FRESHNESS_OUTDIR,
                &ctx.plan_consumer_env,
                ctx.claude_md,
            )?,
            steps::shell_step(
                "Qualify candidate",
                qualify_scrubbed_argv(&spec.qualify)?,
                BTreeMap::new(),
            )?,
        ],
    })
}

fn prepare_candidate_tools_step() -> Result<Step, RenderError> {
    let catalog = ToolCatalog::pinned();
    let homes = ToolHomes::runner_temp();
    let prepare = PreparePinnedTools::new(
        vec![
            PinnedTool::Rust,
            PinnedTool::MrBoxington,
            PinnedTool::Jactionlint,
            PinnedTool::Shellcheck,
        ],
        homes.clone(),
    )
    .map_err(|error| RenderError::BadCommand(error.to_string()))?;
    let argv = prepare
        .argv(&catalog)
        .into_iter()
        .map(OsString::into_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| RenderError::BadCommand("candidate_tools_non_utf8_argv".to_owned()))?;
    let env = env_strings(prepare.env(&catalog))?;
    let mut step = steps::ambient_shell_step("Prepare pinned tools", argv, env)?;
    step.role = Some(StepRole::PreparePinnedTools);
    Ok(step)
}

fn verifier_step(name: &str, uses: &str, role: StepRole) -> Result<Step, RenderError> {
    let mut step = steps::action_step(name, uses, BTreeMap::new())?;
    step.role = Some(role);
    Ok(step)
}

fn owned_tool_env(
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<BTreeMap<String, String>, RenderError> {
    env_strings(homes.exec_env(catalog))
}

fn env_strings(
    entries: impl IntoIterator<Item = (OsString, OsString)>,
) -> Result<BTreeMap<String, String>, RenderError> {
    entries
        .into_iter()
        .map(|(key, value)| {
            Ok((
                key.into_string()
                    .map_err(|_| RenderError::BadCommand("candidate_env_non_utf8".to_owned()))?,
                value
                    .into_string()
                    .map_err(|_| RenderError::BadCommand("candidate_env_non_utf8".to_owned()))?,
            ))
        })
        .collect()
}

fn qualify_scrubbed_argv(argv: &[String]) -> Result<Vec<String>, RenderError> {
    let [shell, flag, _script] = argv else {
        return Err(RenderError::BadCommand("qualify_without_unset".to_owned()));
    };
    if shell != "sh" || flag != "-c" {
        return Err(RenderError::BadCommand("qualify_without_unset".to_owned()));
    }
    Ok(argv.to_vec())
}

pub(crate) fn toolchain_identity(build: &[String]) -> Result<String, RenderError> {
    let mut specs = Vec::new();
    let mut in_specs = false;
    for arg in build {
        if arg == "exec" {
            in_specs = true;
        } else if arg == "--" {
            break;
        } else if in_specs {
            specs.push(arg.clone());
        }
    }
    if specs.is_empty() || specs.iter().any(|spec| !spec.contains('@')) {
        return Err(RenderError::BadCommand(
            "candidate_build_without_toolchain".to_owned(),
        ));
    }
    Ok(specs.join("+"))
}
