//! MBX action ownership and lane selection regressions.

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::timeout::JobTimeout;
use velnor_actions_contract::{Job, Step, StepKind};

use super::{
    EXPORT_SCRIPT, HOSTED_STORE_ISOLATION, MBX_ACTION_CACHE_MODE, MBX_ACTION_NAME,
    MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_IMPORT_NAME, MBX_BUNDLE_KEY_NAME, MBX_BUNDLE_RESTORE_NAME,
    MBX_BUNDLE_SAVE_NAME, MBX_CACHE_MODE_ENV, MBX_RESTORE_NAME, MBX_STORE_INIT_NAME, PREP_IF,
    SAVE_IF, SCALE_SET_ONLY_IF, STORE_INIT_SCRIPT, append_single_bundle_saves,
};

const HOSTED_CACHE_MODE: &str = "${{ runner.environment == 'github-hosted' && 'write' || 'read' }}";
const ACTION_V1_6: &str = "1687e54eb349cadf61fa38b5813a77875489e8e6";
const ACTION_V1_7_1: &str = "d0825fbaf3cc36ca2609aa38e71046265a1f1e37";

fn mbx_job(isolate_hosted: bool) -> Job {
    let with = if isolate_hosted {
        BTreeMap::from([(
            "isolate-objects-cache".to_owned(),
            HOSTED_STORE_ISOLATION.to_owned(),
        )])
    } else {
        BTreeMap::new()
    };
    let mode = if isolate_hosted {
        HOSTED_CACHE_MODE
    } else {
        MBX_ACTION_CACHE_MODE
    };
    let action_sha = if isolate_hosted {
        ACTION_V1_7_1
    } else {
        ACTION_V1_6
    };
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
                uses: format!("{MBX_ACTION_NAME}@{action_sha}"),
                with,
                env: BTreeMap::from([(MBX_CACHE_MODE_ENV.to_owned(), mode.to_owned())]),
            },
        }],
    }
}

fn job_with(isolate_hosted: bool) -> Result<Job, Box<dyn Error>> {
    let mut jobs = BTreeMap::from([("demo".to_owned(), mbx_job(isolate_hosted))]);
    append_single_bundle_saves(&mut jobs)?;
    jobs.remove("demo")
        .ok_or_else(|| std::io::Error::other("rendered MBX job missing").into())
}

#[test]
fn hosted_action_owns_every_hosted_cache_step_and_mode() -> Result<(), Box<dyn Error>> {
    let job = job_with(true)?;
    let init = job
        .steps
        .iter()
        .find(|step| step.name == MBX_STORE_INIT_NAME)
        .ok_or_else(|| std::io::Error::other("private store init missing"))?;
    assert_eq!(init.condition.as_deref(), Some(SCALE_SET_ONLY_IF));
    for name in [
        MBX_BUNDLE_KEY_NAME,
        MBX_BUNDLE_RESTORE_NAME,
        MBX_BUNDLE_IMPORT_NAME,
    ] {
        let step = job
            .steps
            .iter()
            .find(|step| step.name == name)
            .ok_or_else(|| std::io::Error::other("manual MBX restore step missing"))?;
        assert_eq!(step.condition.as_deref(), Some(SCALE_SET_ONLY_IF));
    }
    for (name, gate) in [
        (MBX_BUNDLE_EXPORT_NAME, PREP_IF),
        (MBX_BUNDLE_SAVE_NAME, SAVE_IF),
    ] {
        let step = job
            .steps
            .iter()
            .find(|step| step.name == name)
            .ok_or_else(|| std::io::Error::other("manual MBX writer step missing"))?;
        let expected_condition = format!("{SCALE_SET_ONLY_IF} && {gate}");
        assert_eq!(step.condition.as_deref(), Some(expected_condition.as_str()));
    }
    let StepKind::Action { env, .. } = &job.steps[1].kind else {
        return Err(std::io::Error::other("expected MBX action after private init").into());
    };
    assert_eq!(
        env.get(MBX_CACHE_MODE_ENV).map(String::as_str),
        Some(HOSTED_CACHE_MODE)
    );
    let action_use = match &job.steps[1].kind {
        StepKind::Action { uses, .. } => uses,
        _ => return Err(std::io::Error::other("expected MBX action").into()),
    };
    assert_eq!(action_use, &format!("{MBX_ACTION_NAME}@{ACTION_V1_7_1}"));
    assert!(!STORE_INIT_SCRIPT.is_empty());
    assert!(!EXPORT_SCRIPT.contains("rm -rf"));
    Ok(())
}

#[test]
fn v16_action_keeps_the_single_manual_route_in_both_lanes() -> Result<(), Box<dyn Error>> {
    let job = job_with(false)?;
    let init = job
        .steps
        .iter()
        .find(|step| step.name == MBX_STORE_INIT_NAME)
        .ok_or_else(|| std::io::Error::other("private store init missing"))?;
    assert!(init.condition.is_none());
    for name in [
        MBX_BUNDLE_KEY_NAME,
        MBX_BUNDLE_RESTORE_NAME,
        MBX_BUNDLE_IMPORT_NAME,
    ] {
        let step = job
            .steps
            .iter()
            .find(|step| step.name == name)
            .ok_or_else(|| std::io::Error::other("manual MBX restore step missing"))?;
        assert!(step.condition.is_none());
    }
    let StepKind::Action { env, .. } = &job.steps[1].kind else {
        return Err(std::io::Error::other("expected MBX action after private init").into());
    };
    assert_eq!(
        env.get(MBX_CACHE_MODE_ENV).map(String::as_str),
        Some(MBX_ACTION_CACHE_MODE)
    );
    Ok(())
}

#[test]
fn unknown_action_isolation_mode_fails_closed() -> Result<(), Box<dyn Error>> {
    let mut job = mbx_job(false);
    let StepKind::Action { with, .. } = &mut job.steps[0].kind else {
        return Err(std::io::Error::other("expected action").into());
    };
    with.insert("isolate-objects-cache".to_owned(), "true".to_owned());
    let mut jobs = BTreeMap::from([("demo".to_owned(), job)]);
    assert!(append_single_bundle_saves(&mut jobs).is_err());
    Ok(())
}
