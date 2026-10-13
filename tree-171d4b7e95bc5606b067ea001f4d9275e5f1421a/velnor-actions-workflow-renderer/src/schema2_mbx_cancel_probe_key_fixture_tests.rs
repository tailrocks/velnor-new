//! The cancellation key pin runs the production MBX key step without a matrix.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::super::super::MbxQualificationPins;
use super::super::{Phase, policy, victim_job};
use super::{run_bash, temp_dir};
use crate::mbx_bundle;
use velnor_actions_contract::{PullRequestCachePolicy, Step, StepKind};

const MBX_ACTION: &str = "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6";
const MISE_ACTION: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
const SOURCE_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";
const RUSTC_IDENTITY: &str = "13936cde15db9d31620cb9989927cdfa06948615fc6b8291d7fba92f191a18ec";
const SCOPE_HASH: &str = "4efa592dc896090b6cfbca2010832cd66ffdbd2972b8c791100756fef657c61e";

#[test]
fn production_key_step_emits_exact_dotted_identity_for_pinned_scope() -> Result<(), Box<dyn Error>>
{
    let step = production_key_step()?;
    let StepKind::Shell { run, env } = step.kind else {
        return Err("MBX key step is not a shell step".into());
    };
    assert_eq!(
        env.get("MBX_MATRIX_CONTEXT").map(String::as_str),
        Some("{}")
    );
    assert_eq!(env.get("MBX_BASE_SHA").map(String::as_str), Some(""));
    let script = run.get(2).ok_or("MBX key step has no inline script")?;
    let root = temp_dir("production-key")?;
    let bin = super::fake_bin(&root)?;
    let mise = bin.join("mise");
    fs::write(
        &mise,
        "#!/usr/bin/env bash\nprintf 'rustc 1.98.1\\nhost: x86_64-unknown-linux-gnu\\nrelease: 1.98.1\\n'\n",
    )?;
    fs::set_permissions(&mise, fs::Permissions::from_mode(0o755))?;
    let output = root.join("key.output");
    fs::write(&output, "")?;
    let mut runtime_env = env;
    runtime_env.extend(BTreeMap::from([
        ("MBX_VERSION".to_owned(), "1.22.0".to_owned()),
        ("MBX_BASE_SHA".to_owned(), String::new()),
        ("GITHUB_OUTPUT".to_owned(), output.display().to_string()),
        (
            "GITHUB_WORKFLOW_REF".to_owned(),
            "tailrocks/velnor-new/.github/workflows/qualification.yml@refs/heads/main".to_owned(),
        ),
        ("GITHUB_SHA".to_owned(), SOURCE_SHA.to_owned()),
        ("GITHUB_RUN_ID".to_owned(), "123".to_owned()),
        ("GITHUB_RUN_ATTEMPT".to_owned(), "1".to_owned()),
        ("RUNNER_OS".to_owned(), "Linux".to_owned()),
        ("RUNNER_ARCH".to_owned(), "X64".to_owned()),
    ]));
    let env = runtime_env.into_iter().collect::<Vec<_>>();
    let result = run_bash(script, &root, &bin, &env)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = fs::read_to_string(&output)?;
    assert!(
        text.contains(&format!("rustc_identity={RUSTC_IDENTITY}\n")),
        "{text}"
    );
    let expected = format!(
        "linux-x64-mbx-velnor-mbx-1.22.0-dir-rust-1.98.1-{RUSTC_IDENTITY}-scope-{SCOPE_HASH}-run-123-attempt-1-{SOURCE_SHA}"
    );
    assert!(text.contains(&format!("primary={expected}\n")), "{text}");
    assert_eq!(expected, super::controller_fixtures::expected_key());
    fs::remove_dir_all(root)?;
    Ok(())
}

fn production_key_step() -> Result<Step, Box<dyn Error>> {
    let request = MbxQualificationPins {
        mise_setup: crate::setup::MiseSetup {
            uses: MISE_ACTION.to_owned(),
            version: "2025.9.5".to_owned(),
            sha256: "d".repeat(64),
        },
        mbx_action_uses: MBX_ACTION.to_owned(),
        mbx_version: "1.22.0".to_owned(),
        rust_version: "1.98.1".to_owned(),
    };
    let hosted = crate::yaml::Yaml::str("ubuntu-26.04");
    let job = victim_job(&request, Phase::DuringSave, &hosted)?;
    let mut jobs = BTreeMap::from([(Phase::DuringSave.victim_id().to_owned(), job)]);
    mbx_bundle::append_single_bundle_saves(&mut jobs, PullRequestCachePolicy::ReadOnly)?;
    let Some(mut job) = jobs.remove(Phase::DuringSave.victim_id()) else {
        return Err("MBX victim job missing".into());
    };
    policy::pin_unmatrixed_key_context(&mut job)?;
    job.steps
        .into_iter()
        .find(|step| step.name == mbx_bundle::MBX_BUNDLE_KEY_NAME)
        .ok_or_else(|| "MBX key step missing".into())
}
