//! MBX cache ownership and version compatibility regressions.

use std::collections::BTreeMap;
use std::error::Error;

use crate::cache_steps::{MBX_ACTION_CACHE_MODE, MBX_CACHE_MODE_ENV};
use velnor_actions_contract::workflow::timeout::JobTimeout;
use velnor_actions_contract::{Job, Step, StepKind};

use super::{
    MBX_ACTION_NAME, MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_IMPORT_NAME, MBX_BUNDLE_KEY_NAME,
    MBX_BUNDLE_PATH, MBX_BUNDLE_RESTORE_NAME, MBX_BUNDLE_SAVE_NAME, MBX_RESTORE_NAME,
    MBX_STORE_INIT_NAME, SCALE_SET_ONLY_IF, STORE_INIT_SCRIPT, append_single_bundle_saves, lane,
};

const ACTION_V1_6: &str = "1687e54eb349cadf61fa38b5813a77875489e8e6";
const HOSTED_BACKEND: &str = "${{ runner.environment == 'github-hosted' && 'github' || 'local' }}";
const HOSTED_CACHE_MODE: &str = "${{ runner.environment == 'github-hosted' && github.event_name == 'push' && 'write' || 'read' }}";

fn mbx_job() -> Job {
    Job {
        display_name: "MBX job".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: MBX_RESTORE_NAME.to_owned(),
            condition: None,
            kind: StepKind::Action {
                uses: format!("{MBX_ACTION_NAME}@{ACTION_V1_6}"),
                with: BTreeMap::from([
                    ("github-cache-mode".to_owned(), "objects".to_owned()),
                    ("toolchain".to_owned(), "1.98.1".to_owned()),
                    (
                        "cache-generation".to_owned(),
                        "velnor-mbx-1.21.1".to_owned(),
                    ),
                ]),
                env: BTreeMap::from([(
                    MBX_CACHE_MODE_ENV.to_owned(),
                    MBX_ACTION_CACHE_MODE.to_owned(),
                )]),
            },
        }],
    }
}

fn job_with_bundle_route() -> Result<Job, Box<dyn Error>> {
    let mut jobs = BTreeMap::from([("demo".to_owned(), mbx_job())]);
    append_single_bundle_saves(&mut jobs)?;
    jobs.remove("demo")
        .ok_or_else(|| std::io::Error::other("rendered MBX job missing").into())
}

#[test]
fn v16_action_routes_hosted_cache_to_github_and_scaleset_to_manual_bundle()
-> Result<(), Box<dyn Error>> {
    let job = job_with_bundle_route()?;
    let action_index = job
        .steps
        .iter()
        .position(|step| step.name == MBX_RESTORE_NAME)
        .ok_or_else(|| std::io::Error::other("MBX action missing"))?;
    let StepKind::Action { uses, with, env } = &job.steps[action_index].kind else {
        return Err(std::io::Error::other("MBX restore is not an action").into());
    };
    assert_eq!(uses, &format!("{MBX_ACTION_NAME}@{ACTION_V1_6}"));
    assert_eq!(
        with.get("backend").map(String::as_str),
        Some(HOSTED_BACKEND)
    );
    assert!(!with.contains_key("isolate-objects-cache"));
    assert_eq!(
        env.get(MBX_CACHE_MODE_ENV).map(String::as_str),
        Some(HOSTED_CACHE_MODE)
    );

    let init = job
        .steps
        .iter()
        .find(|step| step.name == MBX_STORE_INIT_NAME)
        .ok_or_else(|| std::io::Error::other("private store init missing"))?;
    assert_eq!(init.condition.as_deref(), Some(SCALE_SET_ONLY_IF));
    let key = job
        .steps
        .iter()
        .find(|step| step.name == MBX_BUNDLE_KEY_NAME)
        .ok_or_else(|| std::io::Error::other("manual MBX key step missing"))?;
    assert_eq!(key.condition.as_deref(), Some(SCALE_SET_ONLY_IF));
    for (name, required) in [
        (MBX_BUNDLE_RESTORE_NAME, "steps.mbx-bundle-key.outcome"),
        (MBX_BUNDLE_IMPORT_NAME, "steps.mbx-bundle.outcome"),
    ] {
        let step = job
            .steps
            .iter()
            .find(|step| step.name == name)
            .ok_or_else(|| std::io::Error::other("manual MBX restore step missing"))?;
        let condition = step
            .condition
            .as_deref()
            .ok_or_else(|| std::io::Error::other("manual MBX step must be gated"))?;
        assert!(condition.starts_with(SCALE_SET_ONLY_IF));
        assert!(condition.contains(required), "{condition}");
        if name == MBX_BUNDLE_IMPORT_NAME {
            assert!(condition.contains("steps.mbx-bundle-key.outputs.ready == 'true'"));
        }
    }
    for name in [MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_SAVE_NAME] {
        let step = job
            .steps
            .iter()
            .find(|step| step.name == name)
            .ok_or_else(|| std::io::Error::other("manual MBX writer step missing"))?;
        assert!(
            step.condition
                .as_deref()
                .is_some_and(|condition| condition.starts_with(SCALE_SET_ONLY_IF))
        );
    }
    let restore_path = match &job
        .steps
        .iter()
        .find(|step| step.name == MBX_BUNDLE_RESTORE_NAME)
        .ok_or_else(|| std::io::Error::other("bundle restore missing"))?
        .kind
    {
        StepKind::Action { with, .. } => with.get("path"),
        _ => None,
    };
    let save_path = match &job
        .steps
        .iter()
        .find(|step| step.name == MBX_BUNDLE_SAVE_NAME)
        .ok_or_else(|| std::io::Error::other("bundle save missing"))?
        .kind
    {
        StepKind::Action { with, .. } => with.get("path"),
        _ => None,
    };
    assert_eq!(restore_path.map(String::as_str), Some(MBX_BUNDLE_PATH));
    assert_eq!(save_path, restore_path);
    assert!(!STORE_INIT_SCRIPT.is_empty());
    Ok(())
}

#[test]
fn pinned_action_generation_matches_its_directory_cache_version() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        lane::directory_cache_generation("velnor-mbx-1.21.1")?,
        "velnor-mbx-1.21.1-dir"
    );
    assert_eq!(
        lane::directory_cache_generation("velnor-mbx-1.11.9")?,
        "velnor-mbx-1.11.9"
    );
    assert!(lane::directory_cache_generation("latest").is_err());
    Ok(())
}
