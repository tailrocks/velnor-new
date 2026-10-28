// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_contract::{
    Job, ProposedTask, REQUIRED_JOB_ID, StepKind, StepRole, WorkflowPolicy,
};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::maintenance_features::owner_feature_repo;
use super::workflow_configuration_fixture;

const CRANE_SPEC: &str = "crane@0.22.1";
const IMAGE_FIXTURE_STEP: &str = "Prepare image publication fixture";

#[test]
fn repository_cli_prepares_crane_before_each_unsharded_nextest_obligation()
-> Result<(), Box<dyn Error>> {
    assert_repository_cli_prepares_crane_before_nextest(false)
}

#[test]
fn repository_cli_prepares_crane_before_each_sharded_nextest_obligation()
-> Result<(), Box<dyn Error>> {
    assert_repository_cli_prepares_crane_before_nextest(true)
}

fn assert_repository_cli_prepares_crane_before_nextest(
    sharded: bool,
) -> Result<(), Box<dyn Error>> {
    let repo = repository_cli_repo(sharded)?;
    let prepared = prepare(repo.path())?;
    let nextest = prepared
        .discovery
        .proposals
        .iter()
        .filter(|task| {
            task.stack_id == "rust"
                && task.task_kind == "nextest"
                && task.identity.unit_path == "Cargo.toml"
        })
        .collect::<Vec<_>>();
    assert_ne!(nextest.len(), 0);
    assert_nextest_partition_shape(&nextest, sharded);

    let jobs = finalized_jobs(&prepared)?;
    let branch = branch_jobs(&jobs, "Rust / velnor-actions-cli")?;
    let required = jobs.get(REQUIRED_JOB_ID).ok_or("Required job is missing")?;
    let task_ids = nextest
        .iter()
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(branch.len(), 2, "expected read and write CLI branches");
    assert_nextest_follows_crane_and_fixture(branch, required, &task_ids, nextest.len())?;
    Ok(())
}

fn assert_nextest_partition_shape(nextest: &[&ProposedTask], sharded: bool) {
    if sharded {
        let partitions = nextest
            .iter()
            .flat_map(|task| task.payload.windows(2))
            .filter(|pair| pair[0] == "--partition")
            .map(|pair| pair[1].to_string_lossy().into_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            partitions,
            BTreeSet::from(["hash:1/2".to_owned(), "hash:2/2".to_owned()])
        );
    } else {
        assert!(
            nextest
                .iter()
                .all(|task| { !task.payload.iter().any(|arg| arg == "--partition") })
        );
    }
}

fn assert_nextest_follows_crane_and_fixture(
    branch: Vec<(&str, &Job)>,
    required: &Job,
    task_ids: &BTreeSet<&str>,
    nextest_count: usize,
) -> Result<(), Box<dyn Error>> {
    for (job_id, job) in branch {
        assert!(
            required.needs.contains(&job_id.to_owned()),
            "Required omitted {job_id}"
        );
        let preparation = assert_crane_prepared(job, true)?;
        let fixture =
            assert_image_publication_fixture(job, true)?.ok_or("fixture step is missing")?;
        assert!(
            preparation < fixture,
            "fixture preparation must follow Crane setup"
        );
        let execution = job
            .steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| match &step.kind {
                StepKind::TaskExecution { task_id, .. } if task_ids.contains(task_id.as_str()) => {
                    Some((index, task_id.as_str()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            execution.len(),
            nextest_count,
            "Nextest tasks missing from {job_id}"
        );
        assert!(
            execution.iter().all(|(index, _)| preparation < *index),
            "Crane preparation must precede every Nextest task in {job_id}"
        );
        assert!(
            execution.iter().all(|(index, _)| fixture < *index),
            "fixture preparation must precede every Nextest task in {job_id}"
        );
    }
    Ok(())
}

#[test]
fn crane_preparation_stays_scoped_to_repository_cli_crate_jobs() -> Result<(), Box<dyn Error>> {
    let cases = [
        (WorkflowPolicy::ConsumerV1, "velnor-actions-cli"),
        (WorkflowPolicy::VelnorRepositoryV1, "demo"),
    ];
    for (policy, package) in cases {
        let repo = owner_feature_repo(policy, true, "default", "default")?;
        rename_root_package(repo.path(), package)?;
        write_project_mise(repo.path())?;
        let prepared = prepare(repo.path())?;
        let jobs = finalized_jobs(&prepared)?;
        for (_, job) in branch_jobs(&jobs, &format!("Rust / {package}"))? {
            assert_crane_prepared(job, false)?;
            assert_image_publication_fixture(job, false)?;
        }
    }

    let repo = repository_cli_repo(false)?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    for (_, job) in branch_jobs(&jobs, "Rust / velnor-actions-cli")? {
        assert_image_publication_fixture(job, true)?;
    }
    let seeds = jobs
        .values()
        .filter(|job| job.is_dependency_seed())
        .collect::<Vec<_>>();
    assert_ne!(seeds.len(), 0);
    for seed in seeds {
        assert_crane_prepared(seed, false)?;
        assert_image_publication_fixture(seed, false)?;
    }

    let repo = prebuilt_image_repo()?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    let image = jobs
        .get("image-demo-amd64")
        .ok_or("prebuilt image job is missing")?;
    assert_crane_prepared(image, false)?;
    assert_image_publication_fixture(image, false)?;
    Ok(())
}

fn branch_jobs<'jobs>(
    jobs: &'jobs std::collections::BTreeMap<String, Job>,
    base: &str,
) -> Result<Vec<(&'jobs str, &'jobs Job)>, Box<dyn Error>> {
    let read = format!("{base} (read)");
    let write = format!("{base} (write)");
    let matched = jobs
        .iter()
        .filter(|(_, job)| {
            job.display_name == base || job.display_name == read || job.display_name == write
        })
        .map(|(id, job)| (id.as_str(), job))
        .collect::<Vec<_>>();
    if matched.is_empty() {
        return Err(format!("{base} job is missing").into());
    }
    Ok(matched)
}

fn assert_crane_prepared(job: &Job, expected: bool) -> Result<usize, Box<dyn Error>> {
    let (index, step) = job
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| step.role == Some(StepRole::PreparePinnedTools))
        .ok_or("pinned tool preparation is missing")?;
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err("pinned tool preparation is not a shell step".into());
    };
    assert_eq!(run.join(" ").contains("crane@"), expected);
    if expected {
        assert!(run.join(" ").contains(CRANE_SPEC));
    }
    Ok(index)
}

fn assert_image_publication_fixture(
    job: &Job,
    expected: bool,
) -> Result<Option<usize>, Box<dyn Error>> {
    let matches = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.name == IMAGE_FIXTURE_STEP)
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), usize::from(expected));
    let Some((index, step)) = matches.first().copied() else {
        return Ok(None);
    };
    let StepKind::Shell { run, env } = &step.kind else {
        return Err("fixture preparation is not a shell step".into());
    };
    let script = run.join(" ");
    let mise_execs = script
        .match_indices("mise --no-config --no-env --no-hooks exec")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(
        mise_execs.len(),
        2,
        "both fixture commands must use pinned Mise"
    );
    let build = script
        .find("-- mbx build --locked")
        .ok_or("fixture example build is not pinned through MBX")?;
    let resolver = script
        .find("-- mbx run --quiet --locked -p velnor-actions-cli --bin velnor-actions")
        .ok_or("native artifact selector is not pinned through MBX")?;
    assert!(mise_execs[0] < build && build < mise_execs[1] && mise_execs[1] < resolver);
    assert!(script.contains("--example image-publication-fixture"));
    assert!(script.contains("--message-format=json"));
    assert!(script.contains("VELNOR_REPO_POLICY_TARGET=image-publication-fixture"));
    assert!(script.contains("VELNOR_REPO_POLICY_ARTIFACT_KIND=example"));
    assert!(script.contains("VELNOR_REPO_POLICY_MANIFEST=crates/velnor-actions-cli/Cargo.toml"));
    assert!(script.contains("mbx run --quiet --locked -p velnor-actions-cli --bin velnor-actions"));
    assert!(script.contains("case \"$fixture\" in /*) test -x \"$fixture\""));
    assert!(script.contains("VELNOR_IMAGE_PUBLICATION_FIXTURE="));
    for key in velnor_actions_workflow_renderer::toolchain_env::STEP_CREDENTIAL_DENYLIST {
        assert_eq!(env.get(key).map(String::as_str), Some(""));
    }
    for key in ["GH_HOST", "GH_CONFIG_DIR"] {
        assert_eq!(env.get(key).map(String::as_str), Some(""));
    }
    Ok(Some(index))
}

fn repository_cli_repo(sharded: bool) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        true,
        "default",
        "default",
    )?;
    rename_root_package(repo.path(), "velnor-actions-cli")?;
    write_project_mise(repo.path())?;
    if sharded {
        let config = repo.path().join(".velnor/config.toml");
        let mut contents = fs::read_to_string(&config)?;
        contents.push_str(
            "\n[test_sharding]\ndefault_shards = 1\n\n[test_sharding.by_manifest]\n\"Cargo.toml\" = 2\n",
        );
        fs::write(config, contents)?;
    }
    Ok(repo)
}

fn prebuilt_image_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        true,
        "default",
        "default",
    )?;
    fs::write(repo.path().join("src/main.rs"), "fn main() {}\n")?;
    fs::write(
        repo.path().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"maintenance\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(
        repo.path().join("Dockerfile"),
        "FROM scratch\nCOPY bin/demo /demo\n",
    )?;
    workflow_configuration_fixture::git(&["add", "--", "Dockerfile"], repo.path())?;
    let config = repo.path().join(".velnor/config.toml");
    let mut contents = fs::read_to_string(&config)?;
    contents.push_str(
        "\n[[images]]\nid = \"demo\"\nrepository = \"ghcr.io/tailrocks/demo\"\ndockerfile = \"Dockerfile\"\nplatforms = [\"linux_amd64\"]\nprebuilt = { kind = \"cargo_bin\", package = \"demo\", bin = \"demo\", stage = \"bin/demo\" }\n",
    );
    fs::write(config, contents)?;
    Ok(repo)
}

fn rename_root_package(root: &Path, name: &str) -> Result<(), Box<dyn Error>> {
    let manifest = root.join("Cargo.toml");
    let contents = fs::read_to_string(&manifest)?;
    fs::write(
        manifest,
        contents.replacen("name = \"demo\"", &format!("name = \"{name}\""), 1),
    )?;
    Ok(())
}

fn write_project_mise(root: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        root.join("mise.toml"),
        include_str!("../../../../mise.toml"),
    )?;
    fs::write(
        root.join("mise.lock"),
        include_str!("../../../../mise.lock"),
    )?;
    Ok(())
}
