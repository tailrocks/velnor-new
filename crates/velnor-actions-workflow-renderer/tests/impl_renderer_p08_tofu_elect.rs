//! P08 `OpenTofu` provider-cache writer-election cases.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_contract::{Job, JobTimeout, Step, StepId, StepKind, StepRole};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::cache_p08::elect_cache_writers;
use velnor_actions_workflow_renderer::tofu_cache::{
    TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDERS_SAVE_USES, tofu_providers_save_step,
};

use super::impl_renderer_fixtures::*;

/// Provider-save steps carried by one job, in step order.
fn provider_saves(job: &Job) -> Vec<&Step> {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::TofuProvidersSave))
        .collect()
}

/// One provider-restore job over an explicit key and path.
fn provider_job(key: &str, path: &str) -> Job {
    let restore = Step {
        name: "Restore Tofu providers".to_owned(),
        id: Some(StepId::TofuProviders),
        role: Some(StepRole::TofuProvidersRestore),
        condition: None,
        kind: StepKind::Action {
            uses: TOFU_PROVIDER_ADMISSION_USES.to_owned(),
            with: BTreeMap::from([
                ("cache-key".to_owned(), key.to_owned()),
                ("cache-path".to_owned(), path.to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    };
    Job {
        display_name: "Provider".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![restore],
    }
}

/// Build a provider save carrying the elected push-only gate.
fn provider_save() -> Result<Step, RenderError> {
    let mut save = tofu_providers_save_step()?;
    save.condition = Some(CACHE_SAVE_CONDITION.to_owned());
    Ok(save)
}

/// Key archived by a job's single provider-save step, when exactly one.
fn provider_saved_key(job: &Job) -> Option<&str> {
    let saves = provider_saves(job);
    if saves.len() != 1 {
        return None;
    }
    match &saves[0].kind {
        StepKind::Action { with, .. } => with.get("key").map(String::as_str),
        _ => None,
    }
}

const PROVIDER_KEY_A: &str = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-b3-0000000000000000000000000000000000000000000000000000000000000000-${{hashFiles('.terraform.lock.hcl')}}";
const PROVIDER_PATH_A: &str = "${{ runner.temp }}/velnor/tofu-cache/b3-0000000000000000000000000000000000000000000000000000000000000000";
const PROVIDER_KEY_B: &str = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-b3-1111111111111111111111111111111111111111111111111111111111111111-${{hashFiles('stacks/vpc/.terraform.lock.hcl')}}";
const PROVIDER_PATH_B: &str = "${{ runner.temp }}/velnor/tofu-cache/b3-1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn provider_writer_election_elects_lowest_id_per_key() -> Result<(), RenderError> {
    let bare = Job {
        display_name: "Plan".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
    };
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), bare),
        (
            "tofu-b".to_owned(),
            provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A),
        ),
        (
            "tofu-a".to_owned(),
            provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A),
        ),
        (
            "tofu-c".to_owned(),
            provider_job(PROVIDER_KEY_B, PROVIDER_PATH_B),
        ),
    ]);
    elect_cache_writers(&mut jobs)?;
    assert_eq!(
        provider_saved_key(&jobs["tofu-a"]),
        Some(velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_KEY_OUTPUT_EXPR),
        "lowest id wins with the key bound to its composite output"
    );
    assert!(provider_saves(&jobs["tofu-b"]).is_empty());
    assert_eq!(
        provider_saved_key(&jobs["tofu-c"]),
        Some(velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_KEY_OUTPUT_EXPR),
        "sole owner keeps its writer bound to its composite output"
    );
    assert!(
        jobs["plan"].steps.is_empty(),
        "the plan job never restores so never saves"
    );
    Ok(())
}

#[test]
fn provider_writer_election_saves_push_gated_exact_entry() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([(
        "tofu-a".to_owned(),
        provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A),
    )]);
    elect_cache_writers(&mut jobs)?;
    let saves = provider_saves(&jobs["tofu-a"]);
    assert_eq!(saves.len(), 1, "winner saves once");
    let save = saves[0];
    assert_eq!(save.condition.as_deref(), Some(CACHE_SAVE_CONDITION));
    let StepKind::Action { uses, with, .. } = &save.kind else {
        panic!("save must be an action step");
    };
    assert_eq!(uses, TOFU_PROVIDERS_SAVE_USES);
    assert_eq!(
        with.get("key").map(String::as_str),
        Some(velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_KEY_OUTPUT_EXPR)
    );
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_PATH_OUTPUT_EXPR)
    );
    assert!(
        !with.contains_key("restore-keys"),
        "saves carry no restore keys"
    );
    Ok(())
}

#[test]
fn provider_writer_election_skips_keyless_and_reruns() -> Result<(), RenderError> {
    let bare = Job {
        display_name: "Bare".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
    };
    let mut jobs = BTreeMap::from([
        ("bare".to_owned(), bare),
        (
            "tofu-a".to_owned(),
            provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A),
        ),
    ]);
    elect_cache_writers(&mut jobs)?;
    elect_cache_writers(&mut jobs)?;
    assert!(jobs["bare"].steps.is_empty(), "keyless job untouched");
    assert_eq!(
        provider_saves(&jobs["tofu-a"]).len(),
        1,
        "reruns add no second save"
    );
    Ok(())
}

#[test]
fn provider_writer_election_rejects_mismatched_or_weak_existing_save() -> Result<(), RenderError> {
    let mut wrong_key = provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A);
    let mut key_save = provider_save()?;
    if let StepKind::Action { with, .. } = &mut key_save.kind {
        with.insert("key".to_owned(), PROVIDER_KEY_A.replace("1.13.1", "1.13.2"));
    }
    wrong_key.steps.push(key_save);
    assert!(
        elect_cache_writers(&mut BTreeMap::from([("tofu-a".to_owned(), wrong_key)])).is_err(),
        "an existing save must use the exact restore key"
    );

    let mut wrong_path = provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A);
    let mut path_save = provider_save()?;
    if let StepKind::Action { with, .. } = &mut path_save.kind {
        with.insert("path".to_owned(), PROVIDER_PATH_B.to_owned());
    }
    wrong_path.steps.push(path_save);
    assert!(
        elect_cache_writers(&mut BTreeMap::from([("tofu-a".to_owned(), wrong_path)])).is_err(),
        "an existing save must use the exact restore path"
    );

    let mut weak_gate = provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A);
    let mut gate_save = provider_save()?;
    gate_save.condition = Some("success()".to_owned());
    weak_gate.steps.push(gate_save);
    assert!(
        elect_cache_writers(&mut BTreeMap::from([("tofu-a".to_owned(), weak_gate)])).is_err(),
        "an existing save must keep the exact push gate"
    );

    let mut duplicate = provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A);
    duplicate.steps.push(provider_save()?);
    duplicate.steps.push(provider_save()?);
    assert!(
        elect_cache_writers(&mut BTreeMap::from([("tofu-a".to_owned(), duplicate)])).is_err(),
        "duplicate provider saves must be rejected"
    );

    let mut matching = provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A);
    matching.steps.push(provider_save()?);
    let mut matching_jobs = BTreeMap::from([("tofu-a".to_owned(), matching)]);
    elect_cache_writers(&mut matching_jobs)?;
    assert_eq!(
        provider_saves(&matching_jobs["tofu-a"]).len(),
        1,
        "a correctly bound pre-existing save stays unique"
    );
    Ok(())
}

#[test]
fn provider_writer_election_rejects_a_valid_save_on_a_losing_owner() -> Result<(), RenderError> {
    let mut losing = provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A);
    losing.steps.push(provider_save()?);
    let mut jobs = BTreeMap::from([
        (
            "tofu-a".to_owned(),
            provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A),
        ),
        ("tofu-b".to_owned(), losing),
    ]);
    let original_steps: BTreeMap<String, Vec<Step>> = jobs
        .iter()
        .map(|(id, job)| (id.clone(), job.steps.clone()))
        .collect();
    assert!(
        elect_cache_writers(&mut jobs).is_err(),
        "only the elected owner can carry the save"
    );
    for (id, steps) in original_steps {
        assert_eq!(jobs[&id].steps, steps, "rejected election mutated {id}");
    }
    Ok(())
}
