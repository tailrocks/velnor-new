// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::ledger_cli::LEDGER_CLI_JOB_ID;
use velnor_actions_contract::{CRATE_JOB_ID_PREFIX, PLAN_JOB_ID, WorkflowPolicy};
use velnor_actions_orchestrator::{WasteFinding, prepare, render_staged_tree, waste_findings};

use super::maintenance_features::owner_feature_repo;
use super::tools_producers::{POLICIES, finalized};
use super::tools_producers_donors::SCHEDULED_SUITES_CONFIG;
use super::waste_lint::{CI_PATH, plain_shell_step, run_without_homes};
use super::waste_oracle::{self as oracle, Files, JobUse, files_of, files_of_repo};
use super::waste_verifier_oracle::{VerifierScan, verifier_homes};
use super::workflow_configuration_fixture;

const FRESHNESS_PATH: &str = ".github/workflows/freshness.yml";
const UNCOVERED_WORKFLOWS: [&str; 6] = [
    "job-image-candidate.yml",
    "product-release-binary.yml",
    "product-release-generator.yml",
    "product-release-images.yml",
    "product-release.yml",
    "qualification.yml",
];

fn assert_no_violations(label: &str, jobs: &[JobUse]) {
    assert!(
        jobs.iter().any(JobUse::is_user),
        "{label}: no toolchain job"
    );
    let found = oracle::violations(jobs);
    assert!(found.is_empty(), "{label}: {found:#?}");
}

fn assert_velnor_repository_guards(files: &Files, jobs: &[JobUse]) -> Result<(), Box<dyn Error>> {
    for path in [CI_PATH, FRESHNESS_PATH] {
        assert!(files.contains_key(path), "{path} is not rendered");
    }
    for id in [
        "cargo-machete",
        "cargo-deny",
        "freshness",
        LEDGER_CLI_JOB_ID,
        PLAN_JOB_ID,
    ] {
        oracle::find(jobs, id)?;
    }
    for id in ["freshness", LEDGER_CLI_JOB_ID, PLAN_JOB_ID] {
        assert!(
            oracle::find(jobs, id)?.is_user(),
            "{id} is not a toolchain job"
        );
    }
    assert!(
        jobs.iter()
            .any(|job| job.job.starts_with(CRATE_JOB_ID_PREFIX) && job.is_user()),
        "no crate job uses the toolchain"
    );
    let deny = oracle::find(jobs, "cargo-deny")?;
    assert!(
        !deny.installs_rust && !deny.words.is_empty(),
        "cargo-deny no longer exercises the clean-cwd exemption: {deny:?}"
    );
    Ok(())
}

#[test]
fn every_toolchain_job_in_the_rendered_trees_has_auto_install_off() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        let prepared = prepare(repo.path())?;
        let files = files_of(&render_staged_tree(&prepared)?)?;
        let label = format!("{policy:?}");
        let jobs = oracle::scan(&files, |_| true)?;
        if policy == WorkflowPolicy::VelnorRepositoryV1 {
            assert_velnor_repository_guards(&files, &jobs)?;
        }
        assert_no_violations(&label, &jobs);
    }
    let repo = workflow_configuration_fixture::make_repo(SCHEDULED_SUITES_CONFIG)?;
    let files = files_of(&render_staged_tree(&prepare(repo.path())?)?)?;
    assert_no_violations("suites", &oracle::scan(&files, |_| true)?);
    Ok(())
}

fn assert_verifiers_read_the_installed_homes(
    label: &str,
    files: &Files,
) -> Result<VerifierScan, Box<dyn Error>> {
    let scan = verifier_homes(files)?;
    assert!(!scan.checked.is_empty(), "{label}: no Rust verifier found");
    assert_eq!(scan.skipped, Vec::<String>::new(), "{label}");
    assert_eq!(scan.violations, Vec::<String>::new(), "{label}");
    Ok(scan)
}

#[test]
fn every_rust_verifier_reads_the_toolchain_home_its_job_installs_into() -> Result<(), Box<dyn Error>>
{
    for policy in POLICIES {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        let files = files_of(&render_staged_tree(&prepare(repo.path())?)?)?;
        let scan = assert_verifiers_read_the_installed_homes(&format!("{policy:?}"), &files)?;
        if policy == WorkflowPolicy::VelnorRepositoryV1 {
            let has = |file: &str, job: &str| {
                scan.checked
                    .iter()
                    .any(|checked| checked.file == file && checked.job == job)
            };
            assert!(has(FRESHNESS_PATH, "freshness"), "{:?}", scan.checked);
            assert!(has(CI_PATH, PLAN_JOB_ID), "{:?}", scan.checked);
            assert!(
                scan.checked
                    .iter()
                    .any(|checked| checked.via_composite
                        && checked.job.starts_with(CRATE_JOB_ID_PREFIX)),
                "{:?}",
                scan.checked
            );
        }
    }
    let repo = workflow_configuration_fixture::make_repo(SCHEDULED_SUITES_CONFIG)?;
    let files = files_of(&render_staged_tree(&prepare(repo.path())?)?)?;
    assert_verifiers_read_the_installed_homes("suites", &files)?;
    Ok(())
}

fn unbound_finding(id: &str) -> WasteFinding {
    WasteFinding::AutoInstallNotDisabled {
        job: id.to_owned(),
        step: None,
        found: None,
    }
}

const MACHETE_SPEC: &str =
    "'http:cargo-machete[url=https://example.invalid/m.tgz,checksum=sha256:00]@0.9.2'";

#[test]
fn w3_flags_a_job_by_the_toolchain_command_its_steps_run() -> Result<(), Box<dyn Error>> {
    let (_, mut jobs) = finalized(WorkflowPolicy::VelnorRepositoryV1)?;
    let (id, at) = plain_shell_step(&jobs)?;
    assert!(!waste_findings(&jobs).contains(&unbound_finding(&id)));
    let flagged: [&[&str]; 5] = [
        &[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            MACHETE_SPEC,
            "--",
            "cargo",
            "machete",
            "crates/a",
        ],
        &["rustup", "show"],
        &["rustc", "--version"],
        &["mbx", "build"],
        &["bash", "-c", "cd work && cargo test"],
    ];
    let clean: [&[&str]; 2] = [
        &[
            "mise",
            "--no-config",
            "exec",
            MACHETE_SPEC,
            "--",
            "cargo-machete",
            "crates/a",
        ],
        &["echo", "cargo-deny", "$CARGO_HOME", "Cargo.toml"],
    ];
    for command in flagged {
        run_without_homes(&mut jobs, &id, at, command)?;
        assert!(
            waste_findings(&jobs).contains(&unbound_finding(&id)),
            "{command:?} was not flagged"
        );
    }
    for command in clean {
        run_without_homes(&mut jobs, &id, at, command)?;
        assert!(
            !waste_findings(&jobs).contains(&unbound_finding(&id)),
            "{command:?} was flagged"
        );
    }
    Ok(())
}

#[test]
fn committed_generated_workflows_have_auto_install_off_or_are_listed_uncovered()
-> Result<(), Box<dyn Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files = files_of_repo(&root)?;
    let scanned = |path: &str| {
        path != CI_PATH
            && !UNCOVERED_WORKFLOWS
                .iter()
                .any(|name| path.ends_with(&format!("/{name}")))
    };
    let jobs = oracle::scan(&files, scanned)?;
    assert!(
        jobs.iter().any(|job| job.file == FRESHNESS_PATH),
        "freshness.yml is not scanned"
    );
    let found = oracle::violations(&jobs);
    assert!(found.is_empty(), "{found:#?}");
    for name in UNCOVERED_WORKFLOWS {
        let path = format!(".github/workflows/{name}");
        let listed = oracle::scan(&files, |candidate| candidate == path)?;
        assert!(
            !oracle::violations(&listed).is_empty(),
            "{path} is covered now; delist it from UNCOVERED_WORKFLOWS"
        );
    }
    Ok(())
}

#[test]
fn rust_auto_install_guard_interprets_yaml_string_values() -> Result<(), Box<dyn Error>> {
    for (value, expected) in [
        ("\"0\"", true),
        ("'0'", true),
        ("\"1\"", false),
        ("0", false),
    ] {
        let files = Files::from([(
            ".github/workflows/example.yml".to_owned(),
            format!(
                "name: Example\non: push\njobs:\n  build:\n    runs-on: ubuntu-24.04\n    env:\n      RUSTUP_AUTO_INSTALL: {value}\n    steps:\n      - run: mise install rust@1.99.1\n      - run: cargo --version\n"
            ),
        )]);
        let jobs = oracle::scan(&files, |_| true)?;
        assert_eq!(oracle::violations(&jobs).is_empty(), expected, "{value}");
    }
    Ok(())
}

#[test]
fn rust_auto_install_guard_scans_aliased_steps_and_runs() -> Result<(), Box<dyn Error>> {
    let workflow = "name: Example\non: push\njobs:\n  first:\n    runs-on: ubuntu-24.04\n    steps:\n      - &s1\n        run: &r1 cargo --version\n  second:\n    runs-on: ubuntu-24.04\n    steps:\n      - *s1\n      - run: *r1\n";
    let files = Files::from([(
        ".github/workflows/example.yml".to_owned(),
        workflow.to_owned(),
    )]);
    let jobs = oracle::scan(&files, |_| true)?;
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().all(|job| job.is_user() && !job.env_off));
    Ok(())
}
