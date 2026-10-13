use std::collections::BTreeMap;

use super::super::share_lanes;
use super::{ctx, echo_step, paired, render_jobs, workflow_ir};
use velnor_actions_contract_workflow::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_contract_workflow::{Step, StepId, StepKind, StepRole};
use velnor_actions_workflow_cache::tofu_cache::{
    TOFU_PROVIDER_ADMISSION_USES, tofu_providers_save_step,
};

const CAP: usize = 500_000;
const LOGICAL_JOBS: usize = 21;
const STEPS_PER_JOB: usize = 48;

fn positions(yaml: &str, needle: &str) -> Vec<usize> {
    yaml.match_indices(needle)
        .map(|(position, _)| position)
        .collect()
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
