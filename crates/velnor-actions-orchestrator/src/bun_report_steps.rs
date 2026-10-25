use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_workflow_renderer::shell_step;
use velnor_actions_workflow_renderer::verification_jobs::{
    BUN_REPORT_DIR, NOT_CANCELLED, bun_blobs_download_step, bun_registry_step,
    bun_report_upload_step,
};

use super::bun_steps::{
    LegArgs, MemberInputs, PW_BLOB_DIR, PW_PROJECT_ENV, PW_SHARD_ENV, bash, bun_words, dir_arg,
    install_step, lifecycle_step, prepare_step, step_env, verify_step,
};
use crate::OrchestratorError;
use crate::workflow_task_jobs::stack_family_cache;

pub(crate) const PW_LEG_ENV: &str = "VELNOR_PW_LEG";
const VERIFY_NAME: &str = "Verify Playwright blob";
const MERGE_NAME: &str = "Merge Playwright reports";

const VERIFY_JS: &str = concat!(
    "const t=await Bun.stdin.text();",
    "const e=t.split(\"\\n\").filter(l=>l.length>0).map(l=>JSON.parse(l));",
    "const f=c=>{console.error(\"velnor: \"+c);process.exit(1)};",
    "const m=e[0];",
    "if(!m||m.method!==\"onBlobReportMetadata\")f(\"playwright_blob_missing:no_metadata\");",
    "if(m.params.version!==2)f(\"playwright_blob_version:\"+m.params.version);",
    "const s=m.params.shard;const got=s?s.current+\"/\"+s.total:\"\";",
    "const want=process.env.VELNOR_PW_SHARD||\"\";",
    "if(got!==want)f(\"playwright_blob_shard_mismatch:\"+want+\":\"+got);",
    "const p=process.env.VELNOR_PW_PROJECT||\"\";",
    "const r=e.filter(x=>x.method===\"onProject\").map(x=>x.params.project);",
    "const n=[...new Set(r.map(x=>x.name))].sort();",
    "if(r.length!==n.length)f(\"playwright_blob_project_mismatch:\"+p+\":\"+n.join(\",\"));",
    "if(n.length===0&&want===\"\")f(\"playwright_blob_project_mismatch:\"+p+\":empty_unsharded_blob\");",
    "if(p!==\"\"&&n.length>0){",
    "const by=new Map(r.map(x=>[x.name,x]));const a=new Set([p]);",
    "if(!by.has(p))f(\"playwright_blob_project_mismatch:\"+p+\":\"+n.join(\",\"));",
    "for(const k of a){const x=by.get(k);if(!x)continue;",
    "const d=x.dependencies||[];if(!Array.isArray(d))f(\"playwright_blob_project_mismatch:\"+p+\":bad_dependencies\");",
    "for(const v of d){if(typeof v!==\"string\")f(\"playwright_blob_project_mismatch:\"+p+\":bad_dependencies\");a.add(v);}",
    "if(x.teardown!==undefined){if(typeof x.teardown!==\"string\")f(\"playwright_blob_project_mismatch:\"+p+\":bad_teardown\");a.add(x.teardown);}}",
    "if(n.some(x=>!a.has(x)))f(\"playwright_blob_project_mismatch:\"+p+\":\"+n.join(\",\"));}",
    "console.log(\"velnor: blob verified shard=\"+(got||\"none\")+\" projects=\"+(n.join(\",\")||\"none\"));",
);

pub(crate) fn blob_verify_step(
    input: &MemberInputs<'_>,
    leg: &LegArgs,
) -> Result<Step, OrchestratorError> {
    let check = bun_words(input.catalog, &["-e".to_owned(), VERIFY_JS.to_owned()])?;
    let body = format!(
        "set -euo pipefail; f=\"$RUNNER_TEMP/{PW_BLOB_DIR}/leg-${PW_LEG_ENV}.zip\"; \
if [ ! -f \"$f\" ] || [ -L \"$f\" ]; then echo 'velnor: playwright_blob_missing: add the blob reporter to the Playwright config under CI' >&2; exit 1; fi; \
unzip -p \"$f\" report.jsonl | {{ grep -E '^[{{]\"method\":\"(onBlobReportMetadata|onProject)\"' || true; }} | {check}"
    );
    let mut env = step_env(input.catalog)?;
    env.insert(PW_LEG_ENV.to_owned(), "${{ matrix.leg }}".to_owned());
    if leg.projects {
        env.insert(
            PW_PROJECT_ENV.to_owned(),
            "${{ matrix.project }}".to_owned(),
        );
    }
    if leg.sharded {
        env.insert(
            PW_SHARD_ENV.to_owned(),
            "${{ matrix.shard }}/${{ matrix.total }}".to_owned(),
        );
    }
    let mut step = shell_step(VERIFY_NAME, bash(&body), env)?;
    step.condition = Some(NOT_CANCELLED.to_owned());
    Ok(step)
}

pub(crate) fn report_steps(
    input: &MemberInputs<'_>,
    suite_job_id: &str,
    retention_days: u16,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::new();
    if !input.scopes.is_empty() || !input.handoff.is_empty() {
        steps.extend(input.acquire.cloned());
    }
    steps.extend(input.handoff.iter().cloned());
    steps.push(prepare_step(input.catalog)?);
    steps.push(verify_step(input)?);
    let bun = stack_family_cache::bun_install(input.root)?;
    steps.extend(stack_family_cache::prelude(std::slice::from_ref(&bun))?);
    if input.scopes.is_empty() {
        steps.push(install_step(input)?);
    } else {
        steps.push(bun_registry_step(
            input.root,
            input.scopes,
            &step_env(input.catalog)?,
        )?);
        steps.push(lifecycle_step(input)?);
    }
    steps.extend(stack_family_cache::after_producer(&bun)?);
    steps.push(bun_blobs_download_step(suite_job_id)?);
    steps.push(merge_step(input)?);
    steps.push(bun_report_upload_step(suite_job_id, retention_days)?);
    Ok(steps)
}

fn merge_step(input: &MemberInputs<'_>) -> Result<Step, OrchestratorError> {
    let run = bun_words(
        input.catalog,
        &[
            "run".to_owned(),
            format!("--cwd={}", dir_arg(&input.member.dir)),
            "playwright".to_owned(),
            "merge-reports".to_owned(),
            "--reporter=html,json".to_owned(),
            "@BLOBS@".to_owned(),
        ],
    )?
    .replace("'@BLOBS@'", "\"$RUNNER_TEMP/velnor/pw-blobs\"");
    let body = format!("set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; {run}");
    let mut env: BTreeMap<String, String> = step_env(input.catalog)?;
    env.extend([
        (
            "PLAYWRIGHT_HTML_OUTPUT_DIR".to_owned(),
            format!("{BUN_REPORT_DIR}/html"),
        ),
        (
            "PLAYWRIGHT_JSON_OUTPUT_FILE".to_owned(),
            format!("{BUN_REPORT_DIR}/report.json"),
        ),
        ("PLAYWRIGHT_HTML_OPEN".to_owned(), "never".to_owned()),
    ]);
    Ok(shell_step(MERGE_NAME, bash(&body), env)?)
}
