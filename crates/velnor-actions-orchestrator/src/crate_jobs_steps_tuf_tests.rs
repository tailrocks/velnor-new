// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::gh_fixture_step;
use velnor_actions_contract::cachekey::ToolchainInputs;
use velnor_actions_contract::{CrateJob, CrateObligation, StepKind, WorkflowPolicy};

fn stage(package: &str, kind: &str, run: &[&str]) -> Vec<String> {
    let job = CrateJob {
        job_id: String::new(),
        display_name: String::new(),
        package_name: package.to_owned(),
        package_id: String::new(),
        manifest: String::new(),
        configuration: String::new(),
        obligations: vec![CrateObligation {
            task_id: String::new(),
            kind: kind.to_owned(),
            step_name: String::new(),
            gated_by: Vec::new(),
            matrix_key: String::new(),
            task_digest: String::new(),
            toolchain_inputs: ToolchainInputs {
                tools: Vec::new(),
                components: Vec::new(),
                compile_driver: "cargo".to_owned(),
                test_runner: if kind == "test" {
                    "cargo"
                } else {
                    "cargo_nextest"
                }
                .to_owned(),
            },
            run: run.iter().map(|arg| (*arg).to_owned()).collect(),
        }],
    };
    let step = gh_fixture_step(WorkflowPolicy::VelnorRepositoryV1, &job)
        .expect("valid staging step")
        .expect("fixture staging");
    let StepKind::Shell { run, .. } = step.kind else {
        panic!("expected shell step");
    };
    run
}

#[test]
fn image_tuf_is_staged_for_test_runs_but_not_binary_listing() {
    let binary_list = stage("velnor-actions-cli", "build", &["cargo", "nextest", "list"]);
    assert!(!binary_list.iter().any(|arg| arg == "--with-image-tuf"));
    for args in [
        vec!["cargo", "nextest", "run"],
        vec!["cargo", "nextest", "run", "--run-ignored", "only"],
        vec!["cargo", "nextest", "run", "--partition", "hash:1/2"],
    ] {
        let run = stage("velnor-actions-cli", "nextest", &args);
        assert!(run.iter().any(|arg| arg == "--with-image-tuf"));
    }
    let cargo_test = stage("velnor-actions-cli", "test", &["cargo", "test", "--locked"]);
    assert!(cargo_test.iter().any(|arg| arg == "--with-image-tuf"));
    let integration = stage(
        "velnor-actions-cli",
        "test",
        &["cargo", "test", "--lib", "--tests"],
    );
    assert!(integration.iter().any(|arg| arg == "--with-image-tuf"));
    for selector in ["--test", "--test=image_publication_public", "--all-targets"] {
        let selected = stage(
            "velnor-actions-cli",
            "test",
            &["cargo", "test", "--lib", selector],
        );
        assert!(selected.iter().any(|arg| arg == "--with-image-tuf"));
    }
    let library = stage("velnor-actions-cli", "test", &["cargo", "test", "--lib"]);
    assert!(!library.iter().any(|arg| arg == "--with-image-tuf"));
    let other_test = stage(
        "velnor-actions-orchestrator",
        "test",
        &["cargo", "test", "--locked"],
    );
    assert!(!other_test.iter().any(|arg| arg == "--with-image-tuf"));
    let other = stage(
        "velnor-actions-orchestrator",
        "nextest",
        &["cargo", "nextest", "run"],
    );
    assert!(!other.iter().any(|arg| arg == "--with-image-tuf"));
}
