// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use velnor_actions_contract::{StepKind, StepRole, WorkflowPolicy};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator::{
    GenerateOptions, finalized_jobs, generate, prepare, render_staged_tree,
};
use velnor_actions_workflow_renderer::repository_nonrust_fixture_step;

use super::nonrust_tools_roundtrip::{RenderedCommand, run_named};

const JOB: &str = "rust-velnor-actions-orchestrator";
const PACKAGE: &str = "velnor-actions-orchestrator";
const MANIFEST: &str = "crates/velnor-actions-orchestrator/Cargo.toml";
const TOKEN: &str = "VELNOR_FIXTURE_ORIGIN_TOKEN";
const GUARDED_TOKEN: &str = "${{ github.event_name == 'push' && github.token || '' }}";

pub(super) fn root() -> Result<PathBuf, Box<dyn Error>> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}

pub(super) fn launcher() -> Result<RenderedCommand, Box<dyn Error>> {
    let prepared = prepare(&root()?)?;
    let rendered = render_staged_tree(&prepared)?;
    let text = rendered
        .get(".github/workflows/ci.yml")
        .ok_or("no CI workflow")?;
    assert!(text.len() <= 1024 * 1024);
    let workflow: serde_json::Value = serde_saphyr::from_str_with_options(
        text,
        serde_saphyr::options! { budget: serde_saphyr::budget! { enforce_alias_anchor_ratio: false } },
    )?;
    run_named(
        &serde_json::to_string(&workflow["jobs"][JOB])?,
        "Prepare non-Rust tools fixture",
    )
}

fn workflow_jobs(text: &str) -> Result<serde_json::Value, Box<dyn Error>> {
    let workflow: serde_json::Value = serde_saphyr::from_str_with_options(
        text,
        serde_saphyr::options! { budget: serde_saphyr::budget! { enforce_alias_anchor_ratio: false } },
    )?;
    Ok(workflow["jobs"].clone())
}

fn assert_no_token_reaches_checkout_code(jobs: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    let mut checked = 0;
    for (id, job) in jobs.as_object().ok_or("no jobs")? {
        for step in job["steps"].as_array().into_iter().flatten() {
            let runs_checkout_code = step["run"]
                .as_str()
                .is_some_and(|run| run.contains("$GITHUB_WORKSPACE") || run.contains("scripts/"));
            if !runs_checkout_code {
                continue;
            }
            for (key, value) in step["env"].as_object().into_iter().flatten() {
                let Some(value) = value.as_str() else {
                    continue;
                };
                if value.contains("github.token") || value.contains("secrets.") {
                    checked += 1;
                    assert!(
                        value.starts_with("${{ github.event_name == 'push' && "),
                        "{id}: {key} reaches pull-request checkout code"
                    );
                }
            }
        }
    }
    assert!(checked > 0);
    Ok(())
}

#[test]
fn pull_request_checkout_code_never_receives_a_token() -> Result<(), Box<dyn Error>> {
    let rendered = render_staged_tree(&prepare(&root()?)?)?;
    let text = rendered
        .get(".github/workflows/ci.yml")
        .ok_or("no CI workflow")?;
    assert_no_token_reaches_checkout_code(&workflow_jobs(text)?)?;
    assert_no_token_reaches_checkout_code(&workflow_jobs(&fs::read_to_string(
        root()?.join(".github/workflows/ci.yml"),
    )?)?)
}

#[test]
fn repository_fixture_is_emitted_only_in_readonly_own_orchestrator_ci() -> Result<(), Box<dyn Error>>
{
    consumer_receipts()?;
    let prepared = prepare(&root()?)?;
    let jobs = finalized_jobs(&prepared)?;
    let owners: Vec<_> = jobs
        .iter()
        .filter(|(_, job)| {
            job.steps
                .iter()
                .any(|step| step.role == Some(StepRole::RepositoryNonrustFixture))
        })
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(owners, [JOB]);
    assert!(jobs[JOB].permissions.is_none());
    let step = launcher()?;
    assert_eq!(step.env.get(TOKEN).map(String::as_str), Some(GUARDED_TOKEN));
    let mut expected = velnor_actions_workflow_renderer::toolchain_env::credential_scrub();
    expected.insert(TOKEN.to_owned(), GUARDED_TOKEN.to_owned());
    assert_eq!(step.env, expected);
    let output = tempfile::tempdir()?;
    generate(
        &prepared,
        &GenerateOptions {
            output_dir: Some(output.path().to_owned()),
        },
    )?;
    Ok(())
}

fn consumer_receipts() -> Result<(), Box<dyn Error>> {
    for repository in [
        super::bun_routing::make_bun_repo()?,
        super::nonrust_family_fixture::repository()?,
    ] {
        let consumer = render_staged_tree(&prepare(repository.path())?)?;
        let workflow = consumer
            .get(".github/workflows/ci.yml")
            .ok_or("no consumer CI")?;
        assert!(!workflow.contains(TOKEN));
        eprintln!("repository_fixture_consumer_workflow={workflow:?}");
    }
    Ok(())
}

#[test]
fn repository_fixture_scope_and_generic_credential_guards_fail_closed() -> Result<(), Box<dyn Error>>
{
    let catalog = ToolCatalog::pinned();
    for (policy, package, manifest) in [
        (WorkflowPolicy::ConsumerV1, PACKAGE, MANIFEST),
        (WorkflowPolicy::VelnorRepositoryV1, "demo", MANIFEST),
        (WorkflowPolicy::VelnorRepositoryV1, PACKAGE, "Cargo.toml"),
    ] {
        assert!(repository_nonrust_fixture_step(policy, package, manifest, &catalog).is_err());
    }
    for key in [TOKEN, "GH_TOKEN", "OTHER_TOKEN"] {
        assert!(
            velnor_actions_workflow_renderer::shell_step(
                "unsafe",
                vec!["true".to_owned()],
                std::collections::BTreeMap::from([(
                    key.to_owned(),
                    "${{ github.token }}".to_owned()
                )])
            )
            .is_err()
        );
    }
    let fixture = repository_nonrust_fixture_step(
        WorkflowPolicy::VelnorRepositoryV1,
        PACKAGE,
        MANIFEST,
        &catalog,
    )?;
    let (_repo, prepared) =
        super::tools_producers_donors::prepared_for(WorkflowPolicy::VelnorRepositoryV1)?;
    let mut wrong_job = prepared.clone();
    wrong_job
        .workflow
        .ir
        .jobs
        .get_mut("rust-demo")
        .ok_or("no demo job")?
        .steps
        .push(fixture.clone());
    assert!(finalized_jobs(&wrong_job).is_err());
    verify_mutated_fixture_scope()
}

fn verify_mutated_fixture_scope() -> Result<(), Box<dyn Error>> {
    let own = prepare(&root()?)?;
    for case in [
        "expression",
        "extra-env",
        "role",
        "operation",
        "condition",
        "parallel",
        "write-permission",
        "job-pull-write",
        "workflow-pull-write",
    ] {
        let mut changed = own.clone();
        if case == "workflow-pull-write" {
            changed.workflow.ir.permissions.pull_requests =
                velnor_actions_contract::PermissionLevel::Write;
        }
        let job = changed
            .workflow
            .ir
            .jobs
            .get_mut(JOB)
            .ok_or("missing own job")?;
        if case == "write-permission" {
            let permissions = velnor_actions_contract::Permissions {
                contents: velnor_actions_contract::PermissionLevel::Write,
                ..Default::default()
            };
            job.permissions = Some(permissions);
        }
        if case == "job-pull-write" {
            job.permissions = Some(velnor_actions_contract::Permissions {
                pull_requests: velnor_actions_contract::PermissionLevel::Write,
                ..Default::default()
            });
        }
        let step = job
            .steps
            .iter_mut()
            .find(|step| step.role == Some(StepRole::RepositoryNonrustFixture))
            .ok_or("no fixture")?;
        mutate(case, step)?;
        assert!(finalized_jobs(&changed).is_err(), "{case}");
    }
    Ok(())
}

fn mutate(case: &str, step: &mut velnor_actions_contract::Step) -> Result<(), Box<dyn Error>> {
    match case {
        "expression" => {
            if let StepKind::Internal { env, .. } = &mut step.kind {
                env.insert(TOKEN.to_owned(), "${{ secrets.OTHER }}".to_owned());
            }
        }
        "extra-env" => {
            if let StepKind::Internal { env, .. } = &mut step.kind {
                env.insert("VELNOR_SCRIPT_PATH".to_owned(), "other.ts".to_owned());
            }
        }
        "role" => step.role = None,
        "operation" => {
            if let StepKind::Internal { operation, .. } = &mut step.kind {
                "plan-v1".clone_into(operation);
            }
        }
        "condition" => step.condition = None,
        "parallel" => step.parallel = true,
        "write-permission" | "job-pull-write" | "workflow-pull-write" => {}
        _ => return Err("unknown mutation".into()),
    }
    Ok(())
}

#[test]
fn repository_fixture_scripts_select_the_orchestrator() -> Result<(), Box<dyn Error>> {
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(root()?.join(".velnor/config.toml"))?)?;
    let rule = config["stacks"]["rust"]["inputs"]
        .as_array()
        .ok_or("no rules")?
        .iter()
        .find(|rule| {
            rule["packages"].as_array().is_some_and(|packages| {
                packages
                    .iter()
                    .any(|package| package.as_str() == Some(PACKAGE))
            })
        })
        .ok_or("no orchestrator rule")?;
    assert_eq!(rule["complete"].as_bool(), Some(true));
    let globs = rule["globs"]
        .as_array()
        .ok_or("no globs")?
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let repo = super::workflow_configuration_fixture::make_repo(&format!(
        "schema=1\n[workflow]\nname=\"CI\"\ndefault_branch=\"testmain\"\n[stacks.rust]\ncompile_driver=\"cargo\"\n[[stacks.rust.inputs]]\nglobs=[{globs}]\npackages=[\"{PACKAGE}\"]\ncomplete=true\n"
    ))?;
    fs::create_dir_all(repo.path().join("crates/velnor-actions-orchestrator/src"))?;
    fs::rename(
        repo.path().join("src/lib.rs"),
        repo.path()
            .join("crates/velnor-actions-orchestrator/src/lib.rs"),
    )?;
    fs::write(
        repo.path().join(MANIFEST),
        fs::read_to_string(repo.path().join("Cargo.toml"))?
            .replace("name = \"demo\"", &format!("name = \"{PACKAGE}\"")),
    )?;
    fs::write(
        repo.path().join("Cargo.toml"),
        "[workspace]\nmembers=[\"crates/velnor-actions-orchestrator\"]\nresolver=\"2\"\n",
    )?;
    super::scoped_workspace::generate_lockfile(repo.path())?;
    fs::create_dir(repo.path().join("scripts"))?;
    let mut base = super::scoped_workspace::commit(repo.path(), "base fixture script inputs")?;
    for path in [
        "scripts/nonrust-bundle-snappy.test.ts",
        "scripts/nonrust-bundle-snappy.ts",
        "scripts/nonrust-tools-transport.ts",
        "scripts/provision-nonrust-tools-fixture.ts",
    ] {
        fs::write(repo.path().join(path), "\"use strict\";\n")?;
        let head = super::scoped_workspace::commit(repo.path(), "change fixture script")?;
        let run = super::scoped_workspace::plan(repo.path(), "pull_request", Some(&base), &head)?;
        assert_eq!(
            run.jobs()?.get(JOB).map(String::as_str),
            Some("run"),
            "{path}"
        );
        base = head;
    }
    Ok(())
}
