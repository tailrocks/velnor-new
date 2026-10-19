//! Per-package `[stacks.rust.crates.<package>]` cases.

use std::collections::BTreeMap;

use velnor_actions_contract::{ContractError, RustCrateConfig, RustStackConfig, VelnorConfig};

use crate::impl_remed_contract::valid_config;

const FILE: &str = ".velnor/config.toml";

fn with_crate(package: &str, build_jobs: u32) -> VelnorConfig {
    let mut config = valid_config();
    let mut rust = RustStackConfig::default_config();
    rust.crates
        .insert(package.to_owned(), RustCrateConfig { build_jobs });
    config.stacks.rust = Some(rust);
    config
}

fn problem(result: Result<(), ContractError>) -> (String, String) {
    let Err(ContractError::Config {
        key_path, problem, ..
    }) = result
    else {
        return ("<no config error>".to_owned(), String::new());
    };
    (key_path, problem)
}

#[test]
fn build_jobs_within_budget_validates() {
    let mut config = with_crate("platform-preview-app", 1);
    config.resources.compiler_process_budget = 2;
    config.validate(FILE).expect("valid crate cap");
    let at_budget = with_crate(
        "platform_preview_app",
        config.resources.compiler_process_budget,
    );
    at_budget.validate(FILE).expect("cap equal to budget");
}

#[test]
fn build_jobs_zero_and_oversized_fail_closed() {
    for jobs in [0, 65] {
        let (key, why) = problem(with_crate("demo", jobs).validate(FILE));
        assert_eq!(key, "stacks.rust.crates.demo.build_jobs");
        assert!(why.starts_with("must_be_between_1_and_"), "{why}");
    }
}

#[test]
fn build_jobs_above_compiler_budget_fails_closed() {
    let mut config = with_crate("demo", 3);
    config.resources.compiler_process_budget = 2;
    let (key, why) = problem(config.validate(FILE));
    assert_eq!(key, "stacks.rust.crates.demo.build_jobs");
    assert_eq!(why, "exceeds_compiler_process_budget");
}

#[test]
fn hostile_package_names_fail_closed() {
    for name in ["", "a b", "a/b", "${{ x }}", "-j1", "a.b"] {
        let result = RustStackConfig {
            crates: BTreeMap::from([(name.to_owned(), RustCrateConfig { build_jobs: 1 })]),
            ..RustStackConfig::default_config()
        }
        .validate(FILE);
        if name == "-j1" {
            // Leading dash is a legal Cargo package spelling and never reaches argv.
            result.expect("dash package name");
        } else {
            let (key, why) = problem(result);
            assert_eq!(key, "stacks.rust.crates");
            assert!(why.starts_with("bad_package_name:"), "{name}: {why}");
        }
    }
}
