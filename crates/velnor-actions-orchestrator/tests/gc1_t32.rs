// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/crate_jobs_stage.rs"]
mod crate_jobs_stage;

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsString;
use std::io;

use velnor_actions_contract::{
    CachePolicy, IdentityInputs, ProposedTask, ResourceClass, ResourceDemand,
};

use self::crate_jobs_stage::{is_mbx, is_nextest, is_opentofu, is_rust, needs};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn fixture_task(stack: &str, driver: &str, runner: &str) -> ProposedTask {
    ProposedTask {
        task_id: "internal/build/dev".to_owned(),
        stack_id: stack.to_owned(),
        component_id: "demo".to_owned(),
        task_kind: "build".to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Compiler,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: IdentityInputs {
            unit_id: "demo".to_owned(),
            unit_key: "demo".to_owned(),
            unit_path: "crates/demo/Cargo.toml".to_owned(),
            project_root: String::new(),
            target: String::new(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: driver.to_owned(),
            test_runner: runner.to_owned(),
            environment: BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: vec![OsString::from("mbx")],
        display_name: "demo".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: "default".to_owned(),
    }
}

fn check_group_classification() -> Result<(), Box<dyn Error>> {
    let cargo = fixture_task("rust", "cargo", "cargo_test");
    let mbx = fixture_task("rust", "mbx", "cargo_nextest");
    let tofu = fixture_task("tofu", "cargo", "cargo_test");
    if !is_rust(&cargo)
        || is_mbx(&cargo)
        || is_nextest(&cargo)
        || is_opentofu(&cargo)
        || needs(&cargo).mbx
        || needs(&cargo).nextest
    {
        return Err(fail("cargo group misclassified"));
    }
    if !is_rust(&mbx) || !is_mbx(&mbx) || !is_nextest(&mbx) || is_opentofu(&mbx) {
        return Err(fail("mbx group misclassified"));
    }
    if is_rust(&tofu) || !is_opentofu(&tofu) || needs(&tofu).mbx || needs(&tofu).nextest {
        return Err(fail("tofu group misclassified"));
    }
    Ok(())
}

#[test]
fn gc1_t32_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_group_classification()?;
    Ok(())
}
