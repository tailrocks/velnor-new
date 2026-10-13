use std::collections::BTreeMap;

use super::super::share_lanes;
use super::{HOSTED_RUNS, ctx, echo_step, paired, render_jobs, workflow_ir};
use velnor_actions_contract_workflow::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_contract_workflow::{Step, StepId, StepKind, StepRole};
use velnor_actions_workflow_cache::tofu_cache::{
    TOFU_PROVIDER_ADMISSION_USES, tofu_providers_save_step,
};
use velnor_actions_workflow_steps::{MiseSetup, RenderError};

const CAP: usize = 500_000;
const LOGICAL_JOBS: usize = 21;
const STEPS_PER_JOB: usize = 48;

fn positions(yaml: &str, needle: &str) -> Vec<usize> {
    yaml.match_indices(needle)
        .map(|(position, _)| position)
        .collect()
}

fn position(yaml: &str, needle: &str) -> usize {
    yaml.find(needle)
        .unwrap_or_else(|| panic!("rendered YAML is missing {needle:?}"))
}

#[test]
fn shared_lanes_keep_ci_under_github_file_cap() {
    let payload = "a".repeat(400);
    let steps: Vec<_> = (0..STEPS_PER_JOB)
        .map(|index| echo_step(index, &payload))
        .collect();
    let jobs = paired(&steps);
    let ir = workflow_ir();
    let context = ctx();
    let unshared_lanes = super::super::LaneShare {
        jobs: jobs.clone(),
        calls: BTreeMap::new(),
        checkouts: BTreeMap::new(),
        env_steps: BTreeMap::new(),
        prefixes: BTreeMap::new(),
        preludes: BTreeMap::new(),
        postludes: BTreeMap::new(),
        files: Vec::new(),
    };
    let unshared = render_jobs(&ir, &unshared_lanes, &context).expect("unshared");
    assert!(
        unshared.len() > CAP,
        "unshared render has {} bytes",
        unshared.len()
    );

    let shared = share_lanes(&jobs, &context).expect("share");
    let yaml = render_jobs(&ir, &shared, &context).expect("shared");
    assert!(yaml.len() <= CAP, "shared ci.yml has {} bytes", yaml.len());
    assert!(!yaml.contains(&payload));
    assert!(yaml.contains("runs-on: ubuntu-26.04"));
    assert!(yaml.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"));
    assert!(yaml.contains("uses: ./.github/actions/rust-0"));

    let checkouts = positions(&yaml, "uses: actions/checkout@");
    let calls = positions(&yaml, "uses: ./.github/actions/rust-");
    assert_eq!(checkouts.len(), LOGICAL_JOBS * 2);
    assert_eq!(calls.len(), LOGICAL_JOBS * 2);
    assert_eq!(
        yaml.matches("# zizmor: ignore[self-repository]").count(),
        LOGICAL_JOBS * 2
    );
    for (index, (checkout, call)) in checkouts.iter().zip(&calls).enumerate() {
        assert!(checkout < call, "job {index} calls before checkout");
        if let Some(next_checkout) = checkouts.get(index + 1) {
            assert!(call < next_checkout, "shared job sequence interleaved");
        }
    }
    assert_eq!(shared.files.len(), LOGICAL_JOBS);
    for file in &shared.files {
        assert!(
            file.bytes.len() <= CAP,
            "{} has {} bytes",
            file.path,
            file.bytes.len()
        );
        assert!(file.bytes.contains("shell: bash"), "{}", file.path);
        assert!(file.bytes.contains(&payload), "{}", file.path);
        assert!(file.path.ends_with("/action.yml"), "{}", file.path);
    }
}

fn insert_p08_setup(jobs: &mut BTreeMap<String, velnor_actions_contract_workflow::Job>) {
    let setup = MiseSetup {
        uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "a".repeat(64),
    };
    let context = ctx();
    for id in ["rust-0__hosted", "rust-0__local"] {
        let job = jobs.get_mut(id).expect("paired job");
        velnor_actions_workflow_cache::cache_p08::ensure_setup_p08(
            id,
            job,
            &setup,
            true,
            "x86_64-unknown-linux-gnu",
            &context.checkout_uses,
        )
        .expect("valid P08 setup");
    }
}

#[test]
fn hosted_and_scale_set_cache_setup_stays_in_lane_jobs_before_shared_body() {
    let mut jobs = paired(&[echo_step(0, "shared-body")]);
    insert_p08_setup(&mut jobs);

    let shared = share_lanes(&jobs, &ctx()).expect("validated cache preludes share");
    let names = |id: &str| {
        shared.prefixes[id]
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names("rust-0__hosted"),
        [
            "Resolve hosted Mise cache identity",
            "Restore Velnor tool seed",
            "Setup Mise",
        ]
    );
    assert_eq!(names("rust-0__local"), ["Setup Mise"]);
    assert!(
        velnor_actions_workflow_cache::cache_p08::is_canonical_hosted_runtime_identity_step(
            HOSTED_RUNS,
            &shared.prefixes["rust-0__hosted"][0],
        )
    );
    assert!(
        !velnor_actions_workflow_cache::cache_p08::is_canonical_hosted_runtime_identity_step(
            &shared.jobs["rust-0__local"].runs_on,
            &shared.prefixes["rust-0__hosted"][0],
        )
    );
    let action = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("shared body action");
    assert!(action.bytes.contains("shared-body"));
    assert!(!action.bytes.contains("Resolve hosted Mise cache identity"));
    assert!(!action.bytes.contains("Restore Velnor tool seed"));
    assert!(!action.bytes.contains("Setup Mise"));

    let yaml = render_jobs(&workflow_ir(), &shared, &ctx()).expect("workflow renders");
    let hosted_at = yaml.find("runs-on: ubuntu-26.04").expect("hosted job");
    let local_at = yaml
        .find("runs-on: [velnor, ubuntu-26.04-scale-set]")
        .expect("scale-set job");
    let hosted = &yaml[hosted_at..local_at];
    let local = &yaml[local_at..];
    assert!(
        position(hosted, "Resolve hosted Mise cache identity")
            < position(hosted, "Restore Velnor tool seed")
    );
    assert!(position(hosted, "Restore Velnor tool seed") < position(hosted, "Setup Mise"));
    assert!(position(hosted, "Setup Mise") < position(hosted, "uses: ./.github/actions/rust-0"));
    assert!(position(local, "Setup Mise") < position(local, "uses: ./.github/actions/rust-0"));
}

fn assert_tofu_provider_scope(shared: &super::super::LaneShare, hosted: &str) {
    let setup_at = hosted
        .find("uses: ./.github/actions/tofu-provider-prelude-0")
        .expect("shared provider prelude call");
    let restore_at = hosted.find("id: tofu-providers").expect("outer restore id");
    let composite_at = hosted
        .find("uses: ./.github/actions/rust-0")
        .expect("shared lane call");
    let save_at = hosted
        .find("name: Save Tofu providers")
        .expect("elected outer save");
    assert!(setup_at < restore_at && restore_at < composite_at && composite_at < save_at);
    assert_eq!(
        hosted
            .matches("uses: ./.github/actions/tofu-provider-prelude-0")
            .count(),
        LOGICAL_JOBS * 2
    );
    assert!(hosted.contains("steps.tofu-providers.outputs.cache-key"));
    assert!(hosted.contains("steps.tofu-providers.outputs.cache-path"));

    let common = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("shared provider-use action");
    assert!(common.bytes.contains("name: Init for validate"));
    assert!(!common.bytes.contains("id: tofu-providers"));
    assert!(!common.bytes.contains("tofu-provider-admission"));
    let provider_prelude = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/tofu-provider-prelude-0/action.yml")
        .expect("shared typed setup prefix");
    assert!(provider_prelude.bytes.contains("shared-prelude"));
    assert!(!provider_prelude.bytes.contains("id: tofu-providers"));
    assert_eq!(hosted.matches("id: tofu-providers").count(), 42);
}

#[test]
fn noncanonical_hosted_runtime_identity_is_not_peeled() {
    let mut jobs = paired(&[echo_step(0, "shared-body")]);
    insert_p08_setup(&mut jobs);
    let hosted = jobs.get_mut("rust-0__hosted").expect("hosted job");
    let identity = hosted
        .steps
        .iter_mut()
        .find(|step| step.name == "Resolve hosted Mise cache identity")
        .expect("canonical identity step");
    let StepKind::Shell { run, .. } = &mut identity.kind else {
        panic!("identity step is a shell command");
    };
    run[2].push_str("; echo unexpected");

    let error = share_lanes(&jobs, &ctx()).expect_err("mutated identity remains in body");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(ref problem) if problem == "lane_body_differs:rust-0"),
        "{error}"
    );
}

#[test]
fn malformed_typed_mise_setup_is_not_peeled() {
    let mut jobs = paired(&[echo_step(0, "shared-body")]);
    insert_p08_setup(&mut jobs);
    let hosted = jobs.get_mut("rust-0__hosted").expect("hosted job");
    let setup = hosted
        .steps
        .iter_mut()
        .find(|step| step.name == "Setup Mise")
        .expect("Mise setup");
    let StepKind::Action { with, .. } = &mut setup.kind else {
        panic!("Mise setup is an action");
    };
    with.insert("unexpected".to_owned(), "true".to_owned());

    let error = share_lanes(&jobs, &ctx()).expect_err("malformed typed setup remains in body");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(ref problem) if problem == "lane_body_differs:rust-0"),
        "{error}"
    );
}

#[test]
fn malformed_typed_tool_seed_is_not_peeled() {
    let mut jobs = paired(&[echo_step(0, "shared-body")]);
    insert_p08_setup(&mut jobs);
    let hosted = jobs.get_mut("rust-0__hosted").expect("hosted job");
    let seed = hosted
        .steps
        .iter_mut()
        .find(|step| step.name == "Restore Velnor tool seed")
        .expect("tool seed");
    let StepKind::Action { with, .. } = &mut seed.kind else {
        panic!("tool seed is an action");
    };
    with.insert("unexpected".to_owned(), "true".to_owned());

    let error = share_lanes(&jobs, &ctx()).expect_err("malformed typed seed remains in body");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(ref problem) if problem == "lane_body_differs:rust-0"),
        "{error}"
    );
}

#[test]
fn cache_setup_prefix_does_not_hide_unrelated_lane_divergence() {
    let mut jobs = paired(&[echo_step(0, "shared-body")]);
    insert_p08_setup(&mut jobs);
    let hosted = jobs.get_mut("rust-0__hosted").expect("hosted job");
    let body_at = hosted
        .steps
        .iter()
        .position(|step| step.name == "echo 0")
        .expect("common body");
    hosted
        .steps
        .insert(body_at, echo_step(9, "host-only setup"));

    let error = share_lanes(&jobs, &ctx()).expect_err("unrelated divergence remains rejected");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(ref problem) if problem == "lane_body_differs:rust-0"),
        "{error}"
    );
}

#[test]
fn paired_tofu_restore_output_owner_stays_in_outer_job_scope() {
    let cache_path = "${{ runner.temp }}/velnor/tofu-cache/root-0123456789ab";
    let cache_key = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-root-0123456789ab-${{hashFiles('.terraform.lock.hcl')}}";
    let restore = Step {
        name: "Restore Tofu providers".to_owned(),
        id: Some(StepId::TofuProviders),
        role: Some(StepRole::TofuProvidersRestore),
        condition: None,
        kind: StepKind::Action {
            uses: TOFU_PROVIDER_ADMISSION_USES.to_owned(),
            with: BTreeMap::from([
                ("cache-key".to_owned(), cache_key.to_owned()),
                ("cache-path".to_owned(), cache_path.to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    };
    let consumer = Step {
        name: "Init for validate".to_owned(),
        id: None,
        role: Some(StepRole::TofuProviderUse),
        condition: None,
        kind: StepKind::Shell {
            run: vec![
                "tofu".to_owned(),
                "init".to_owned(),
                "-lockfile=readonly".to_owned(),
            ],
            env: BTreeMap::from([
                ("TF_PLUGIN_CACHE_DIR".to_owned(), cache_path.to_owned()),
                (
                    "TF_DATA_DIR".to_owned(),
                    "${{ runner.temp }}/velnor/tofu-data/root-0123456789ab".to_owned(),
                ),
            ]),
        },
    };
    let mut jobs = paired(&[echo_step(0, "shared-prelude"), restore, consumer]);
    let mut save = tofu_providers_save_step().expect("provider save step");
    save.condition = Some(CACHE_SAVE_CONDITION.to_owned());
    jobs.get_mut("rust-0__hosted")
        .expect("hosted owner")
        .steps
        .push(save);

    let shared = share_lanes(&jobs, &ctx()).expect("paired provider lanes share");
    let hosted = render_jobs(&workflow_ir(), &shared, &ctx()).expect("workflow renders");
    assert_tofu_provider_scope(&shared, &hosted);
}

mod static_prefix_tests;
mod static_task_tests;
