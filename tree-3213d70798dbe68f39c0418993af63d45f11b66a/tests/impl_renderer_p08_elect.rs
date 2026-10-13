//! P08 writer-election cases for tools and Tofu-provider cache keys.

use std::collections::BTreeMap;
use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_contract::{Job, JobTimeout, Step, StepId, StepKind, StepRole};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::cache_p08::{
    ToolsCacheInputs, ToolsCachePayload, elect_cache_writers,
};
use velnor_actions_workflow_renderer::steps::{TOOLS_CACHE_PATHS, TOOLS_SAVE_USES};
use velnor_actions_workflow_renderer::tofu_cache::{
    TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDERS_SAVE_USES, tofu_providers_save_step,
};

use super::impl_renderer_fixtures::*;

/// `Save Mise tools` steps carried by one job, in step order.
fn tools_saves(job: &Job) -> Vec<&Step> {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheSave))
        .collect()
}

/// One job with the canonical runtime identity and V2 restore for one tool set.
pub(crate) fn keyed_job(tool: &str) -> Result<Job, RenderError> {
    let specs = [tool.to_owned()];
    let payload = ToolsCachePayload::new(ToolsCacheInputs {
        runs_on: LABEL,
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &mise(),
        tool_specs: &specs,
        rustup_toolchain: None,
        rustup_components: &[],
    })?;
    Ok(Job {
        display_name: "Keyed".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![payload.runtime_prelude_step()?, payload.restore_step()?],
    })
}

#[test]
fn tools_cache_writer_election_prefers_plan_then_lowest_id() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), keyed_job("actionlint@1.7.12")?),
        ("rust-b".to_owned(), keyed_job("actionlint@1.7.12")?),
        ("rust-c".to_owned(), keyed_job("shellcheck@0.11.0")?),
    ]);
    elect_cache_writers(&mut jobs)?;
    assert_eq!(
        saved_key(&jobs["plan"]),
        restore_key(&jobs["plan"]),
        "plan wins shared"
    );
    assert!(tools_saves(&jobs["rust-b"]).is_empty(), "sharer saves none");
    assert_eq!(
        saved_key(&jobs["rust-c"]),
        restore_key(&jobs["rust-c"]),
        "sole owner keeps its writer"
    );

    let mut jobs = BTreeMap::from([
        ("rust-b".to_owned(), keyed_job("actionlint@1.7.12")?),
        ("rust-a".to_owned(), keyed_job("actionlint@1.7.12")?),
    ]);
    elect_cache_writers(&mut jobs)?;
    assert_eq!(
        saved_key(&jobs["rust-a"]),
        restore_key(&jobs["rust-a"]),
        "lowest id wins without plan"
    );
    assert_eq!(
        tools_saves(&jobs["rust-b"]),
        [] as [&velnor_actions_contract::Step; 0]
    );
    Ok(())
}

/// The tools key one job's single save step archives, when exactly one.
fn saved_key(job: &Job) -> Option<&str> {
    let saves = tools_saves(job);
    if saves.len() != 1 {
        return None;
    }
    match &saves[0].kind {
        StepKind::Action { with, .. } => with.get("key").map(String::as_str),
        _ => None,
    }
}

fn restore_key(job: &Job) -> Option<&str> {
    job.steps.iter().find_map(|step| {
        if step.role != Some(StepRole::ToolsCacheRestore) {
            return None;
        }
        let StepKind::Action { with, .. } = &step.kind else {
            return None;
        };
        with.get("key").map(String::as_str)
    })
}

#[test]
fn tools_cache_writer_saves_exact_payload_and_is_push_gated() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), keyed_job("actionlint@1.7.12")?),
        ("rust-b".to_owned(), keyed_job("actionlint@1.7.12")?),
    ]);
    elect_cache_writers(&mut jobs)?;
    let saves = tools_saves(&jobs["plan"]);
    assert_eq!(saves.len(), 1, "winner saves once");
    let save = saves[0];
    assert_eq!(
        save.condition.as_deref(),
        Some(
            "success() && github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && steps.v2.outputs.enabled == 'true'"
        )
    );
    let StepKind::Action { uses, with, .. } = &save.kind else {
        panic!("save must be an action step");
    };
    assert_eq!(uses, TOOLS_SAVE_USES);
    assert_eq!(
        with.get("key").map(String::as_str),
        restore_key(&jobs["plan"])
    );
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(TOOLS_CACHE_PATHS.join("\n").as_str())
    );
    Ok(())
}

#[test]
fn tools_cache_writer_skips_keyless_and_reruns() -> Result<(), RenderError> {
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
        ("rust-a".to_owned(), keyed_job("actionlint@1.7.12")?),
    ]);
    elect_cache_writers(&mut jobs)?;
    elect_cache_writers(&mut jobs)?;
    assert!(jobs["bare"].steps.is_empty(), "keyless job untouched");
    assert_eq!(
        tools_saves(&jobs["rust-a"]).len(),
        1,
        "reruns add no second save"
    );
    Ok(())
}

/// `Save Tofu providers` steps carried by one job, in step order.
fn provider_saves(job: &Job) -> Vec<&Step> {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::TofuProvidersSave))
        .collect()
}

/// One provider-restore job over an explicit key + path.
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

fn provider_save() -> Result<Step, RenderError> {
    let mut save = tofu_providers_save_step()?;
    save.condition = Some(CACHE_SAVE_CONDITION.to_owned());
    Ok(save)
}

/// The provider key one job's single save step archives, when exactly one.
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

const PROVIDER_KEY_A: &str = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-root-0123456789ab-${{hashFiles('.terraform.lock.hcl')}}";
const PROVIDER_PATH_A: &str = "${{ runner.temp }}/velnor/tofu-cache/root-0123456789ab";
const PROVIDER_KEY_B: &str = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-stacks-vpc-abcdef012345-${{hashFiles('stacks/vpc/.terraform.lock.hcl')}}";
const PROVIDER_PATH_B: &str = "${{ runner.temp }}/velnor/tofu-cache/stacks-vpc-abcdef012345";

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
        "lowest id wins the shared key"
    );
    assert_eq!(
        provider_saves(&jobs["tofu-b"]),
        [] as [&velnor_actions_contract::Step; 0]
    );
    assert_eq!(
        provider_saved_key(&jobs["tofu-c"]),
        Some(velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_KEY_OUTPUT_EXPR),
        "sole owner keeps its writer"
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
fn combined_election_rejects_provider_conflict_before_appending_tools_save()
-> Result<(), RenderError> {
    let mut losing_provider = provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A);
    losing_provider.steps.push(provider_save()?);
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), keyed_job("actionlint@1.7.12")?),
        (
            "tofu-a".to_owned(),
            provider_job(PROVIDER_KEY_A, PROVIDER_PATH_A),
        ),
        ("tofu-b".to_owned(), losing_provider),
    ]);
    let original_steps = jobs
        .iter()
        .map(|(id, job)| (id.clone(), job.steps.clone()))
        .collect::<BTreeMap<_, _>>();

    assert!(elect_cache_writers(&mut jobs).is_err());
    for (id, steps) in original_steps {
        assert_eq!(jobs[&id].steps, steps, "failed election mutated {id}");
    }
    assert_eq!(
        tools_saves(&jobs["plan"]),
        [] as [&velnor_actions_contract::Step; 0]
    );
    Ok(())
}

#[test]
fn tools_cache_writer_rejects_restore_without_identity() -> Result<(), RenderError> {
    let mut malformed = keyed_job("actionlint@1.7.12")?;
    malformed.steps.remove(0);
    let mut jobs = BTreeMap::from([("rust-z".to_owned(), malformed)]);
    assert!(elect_cache_writers(&mut jobs).is_err());
    Ok(())
}

#[test]
fn tools_cache_election_binds_restore_to_static_identity_digest() -> Result<(), RenderError> {
    use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
    let mut jobs = BTreeMap::from([
        ("rust-a".to_owned(), keyed_job("actionlint@1.7.12")?),
        ("rust-b".to_owned(), keyed_job("actionlint@1.7.12")?),
    ]);
    // Same canonical restore key, but a retargeted static digest splits the
    // election: each identity elects its own writer.
    let identity = &mut jobs.get_mut("rust-b").expect("second owner").steps[0];
    let StepKind::Action { with, .. } = &mut identity.kind else {
        return Err(RenderError::InvalidWorkflow(
            "test_identity_not_action".to_owned(),
        ));
    };
    with.insert(TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(), "f".repeat(64));
    assert_eq!(restore_key(&jobs["rust-a"]), restore_key(&jobs["rust-b"]));
    elect_cache_writers(&mut jobs)?;
    assert_eq!(tools_saves(&jobs["rust-a"]).len(), 1);
    assert_eq!(tools_saves(&jobs["rust-b"]).len(), 1);
    Ok(())
}
