//! Budget projection over finalized jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, RSDEPS_HELPER_KEY_STEP_EXPR, key_step_expr, rsdeps_key_output,
};
use velnor_actions_contract::{Job, JobTimeout, Step, StepKind, StepRole};

use super::*;
use crate::cache_steps::{SeedCacheUse, seed_cache_step};

const RESTORE: &str = "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";

fn job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Job".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    }
}

fn marker(role: StepRole, with: &[(&str, &str)]) -> Step {
    Step {
        parallel: false,
        name: "x".to_owned(),
        id: None,
        role: Some(role),
        condition: None,
        kind: StepKind::Action {
            uses: "x/y@z".to_owned(),
            with: with
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            env: BTreeMap::new(),
        },
    }
}

fn tools(digest: &str, writer: bool) -> Vec<Step> {
    let mut steps = vec![marker(StepRole::ToolsCacheIdentity, &[("d", digest)])];
    if writer {
        steps.push(marker(
            StepRole::ToolsCacheSave,
            &[("key", "mise-tools-v2-x")],
        ));
    }
    steps
}

fn seed(root: &str, ordinal: usize) -> Step {
    seed_cache_step(
        RESTORE,
        CacheFamily::Rsdeps,
        root,
        SeedCacheUse::Lookup,
        &key_step_expr(&rsdeps_key_output(ordinal)),
        None,
    )
    .expect("seed")
}

#[test]
fn written_tools_identities_sources_and_seed_roots_are_counted_once() {
    let mut plan = tools("a", true);
    plan.extend([seed("", 0), seed("nested", 1), seed("", 0)]);
    plan.push(marker(
        StepRole::CargoSourcesRestore,
        &[("key", "${{steps.cache-keys.outputs.sources_key}}")],
    ));
    let ci = BTreeMap::from([
        ("plan".to_owned(), job(plan)),
        ("rust-a".to_owned(), job(tools("b", true))),
        ("rust-b".to_owned(), job(tools("b", false))),
    ]);
    let helper = seed_cache_step(
        RESTORE,
        CacheFamily::RsdepsHelper,
        "",
        SeedCacheUse::Lookup,
        RSDEPS_HELPER_KEY_STEP_EXPR,
        None,
    )
    .expect("helper");
    let seed_jobs = BTreeMap::from([(
        "cache-seed".to_owned(),
        job([tools("c", false), vec![helper]].concat()),
    )]);
    let projection = project(&[&ci, &seed_jobs]);
    assert_eq!(
        projection,
        BudgetProjection {
            tools_identities: 2,
            sources: true,
            seeds: 3,
        },
        "the seed's unwritten tools identity stores nothing"
    );
    let budget = CacheBudget {
        quota_bytes: 10_000_000_000,
        seed_max_bytes: 4 * 1024 * 1024 * 1024,
    };
    assert_eq!(
        projection.bytes(&budget),
        2 * 400_000_000 + 300_000_000 + 3 * 1_073_741_824
    );
    check(&[&ci, &seed_jobs], &budget).expect("fits");
    let tight = CacheBudget {
        quota_bytes: 5_000_000_000,
        ..budget
    };
    let err = check(&[&ci, &seed_jobs], &tight).expect_err("over budget");
    assert!(err.to_string().contains("cache_budget_exceeded"), "{err}");
    assert!(err.to_string().contains("seed_max_bytes"), "{err}");
}
