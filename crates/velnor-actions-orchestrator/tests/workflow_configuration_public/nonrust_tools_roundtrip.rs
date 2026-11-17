// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error, fs, path::Path};

use serde_json::Value;
use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};

use super::{
    nonrust_tools_fixture::{Fixture, copy_tree},
    tools_cache_verify_hook::{Sandbox, expand_shell},
    workflow_configuration_fixture,
};

const VERIFY: &str = ".github/actions/velnor-tools-cache-verify/action.yml";
const RESTORE: &str = ".github/actions/velnor-tools-cache-restore/action.yml";

pub(super) struct RenderedCommand {
    pub shell: String,
    pub run: String,
    pub env: BTreeMap<String, String>,
}

pub(super) fn run_named(text: &str, name: &str) -> Result<RenderedCommand, Box<dyn Error>> {
    let document: Value = serde_saphyr::from_str(text)?;
    let steps = document
        .get("steps")
        .or_else(|| document.get("runs").and_then(|runs| runs.get("steps")))
        .and_then(Value::as_array)
        .ok_or("no rendered steps")?;
    let step = steps
        .iter()
        .find(|step| step.get("name").and_then(Value::as_str) == Some(name))
        .ok_or("rendered step absent")?;
    let env = step
        .get("env")
        .and_then(Value::as_object)
        .map(|env| {
            env.iter()
                .map(|(key, value)| {
                    Ok((
                        key.clone(),
                        value.as_str().ok_or("non-string environment")?.to_owned(),
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(RenderedCommand {
        shell: step
            .get("shell")
            .and_then(Value::as_str)
            .unwrap_or("bash -e {0}")
            .to_owned(),
        run: step
            .get("run")
            .and_then(Value::as_str)
            .ok_or("no rendered command")?
            .to_owned(),
        env,
    })
}

pub(super) fn execute(
    fixture: &Fixture,
    runner: &Sandbox,
    step: &RenderedCommand,
) -> Result<String, Box<dyn Error>> {
    let mut env = fixture.environment(runner);
    env.extend(
        step.env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    let result = runner.run_with_output(&expand_shell(&step.shell), &step.run, &env)?;
    assert_eq!(
        result.code,
        Some(0),
        "{}\n{}\n{}",
        result.stdout,
        result.stderr,
        step.run
    );
    Ok(result.github_output)
}

#[test]
fn genuine_nonrust_tools_producers_supply_fresh_readers_offline() -> Result<(), Box<dyn Error>> {
    let (fixture, _replay) = Fixture::load()?;
    let repo = workflow_configuration_fixture::make_repo(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[[native_checks]]\nid = \"tap-tree\"\noperation = \"tap_tree\"\n[native_checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n",
    )?;
    fs::remove_file(repo.path().join("Cargo.toml"))?;
    fs::remove_file(repo.path().join("src/lib.rs"))?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    let rendered = render_staged_tree(&prepared)?;
    let text = |path: &str| {
        rendered
            .files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.bytes.as_str())
            .ok_or("missing generated file")
    };
    let producer_verify = run_named(text(VERIFY)?, "Re-verify restored Mise installs")?;
    let reader_verify = run_named(text(RESTORE)?, "Re-verify restored Mise installs")?;
    let jactionlint_verify = run_named(
        text(".github/actions/velnor-verify-jactionlint/action.yml")?,
        "Verify jactionlint",
    )?;
    let seal: Value = serde_saphyr::from_str(text(".github/workflows/state-seal.yml")?)?;
    let runtime = Roundtrip {
        fixture: &fixture,
        producer_verify: &producer_verify,
        reader_verify: &reader_verify,
        jactionlint_verify: &jactionlint_verify,
        seal: &seal,
    };
    let mut observed = std::collections::BTreeSet::new();
    for (id, job) in jobs.iter().filter(|(_, job)| {
        job.steps
            .iter()
            .any(|step| step.role == Some(StepRole::ToolsCacheVerify))
    }) {
        observed.extend(runtime.produce(id, job)?);
    }
    assert_eq!(
        observed,
        std::collections::BTreeSet::from(["gh".to_owned(), "shellcheck".to_owned()])
    );
    Ok(())
}

pub(super) fn restore_declared_paths(
    job: &velnor_actions_contract::Job,
    producer: &Sandbox,
    reader: &Sandbox,
) -> Result<(), Box<dyn Error>> {
    let save = job
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::ToolsCacheSave))
        .ok_or("producer has no save")?;
    let StepKind::Action { with, .. } = &save.kind else {
        return Err("save is not action".into());
    };
    for path in with.get("path").ok_or("save paths absent")?.lines() {
        if let Some(path) = path.strip_prefix("~/") {
            let source = producer.home.path().join(path);
            if source.exists() {
                let restored = reader.home.path().join(path);
                copy_tree(&source, &restored)?;
                relocate_links(&restored, producer.home.path(), reader.home.path())?;
            }
        }
    }
    Ok(())
}

fn relocate_links(dir: &Path, from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            relocate_links(&path, from, to)?;
        } else if kind.is_symlink()
            && let Ok(suffix) = fs::read_link(&path)?.strip_prefix(from)
        {
            let target = to.join(suffix);
            fs::remove_file(&path)?;
            std::os::unix::fs::symlink(target, &path)?;
        }
    }
    Ok(())
}

#[test]
fn restored_absolute_links_follow_the_reader_home() -> Result<(), Box<dyn Error>> {
    let (from, to) = (tempfile::tempdir()?, tempfile::tempdir()?);
    let bins = to.path().join("install/.mise-bins");
    fs::create_dir_all(&bins)?;
    std::os::unix::fs::symlink(from.path().join("install/uv"), bins.join("uv"))?;
    relocate_links(to.path(), from.path(), to.path())?;
    assert_eq!(
        fs::read_link(bins.join("uv"))?,
        to.path().join("install/uv")
    );
    Ok(())
}

struct Roundtrip<'a> {
    fixture: &'a Fixture,
    producer_verify: &'a RenderedCommand,
    reader_verify: &'a RenderedCommand,
    jactionlint_verify: &'a RenderedCommand,
    seal: &'a Value,
}

impl Roundtrip<'_> {
    fn produce(
        &self,
        id: &str,
        job: &velnor_actions_contract::Job,
    ) -> Result<std::collections::BTreeSet<String>, Box<dyn Error>> {
        let Self {
            fixture,
            producer_verify,
            seal,
            ..
        } = self;
        let install = job
            .steps
            .iter()
            .find(|step| step.role == Some(StepRole::PreparePinnedTools))
            .ok_or("producer has no install")?;
        let StepKind::Shell { run, .. } = &install.kind else {
            return Err("producer install is not shell".into());
        };
        let selected: Vec<_> = fixture
            .archives
            .iter()
            .filter(|archive| {
                run.iter()
                    .any(|arg| arg.contains(&format!("{}@{}", archive.tool, archive.version)))
            })
            .collect();
        if selected.len() != fixture.archives.len() {
            return Ok(std::collections::BTreeSet::new());
        }
        let producer = Sandbox::new()?;
        fixture.provision(&producer)?;
        let job_text = serde_json::to_string(
            seal.get("jobs")
                .and_then(|jobs| jobs.get(id))
                .ok_or("producer absent from workflow")?,
        )?;
        let recipe = run_named(&job_text, "Prepare pinned tools")?;
        let started = std::time::Instant::now();
        execute(fixture, &producer, &recipe)?;
        execute(fixture, &producer, self.jactionlint_verify)?;
        let derived = producer
            .mise_root()
            .join("installs/jactionlint/2.4.0/.mise-bins");
        assert!(derived.is_dir());
        let receipt = execute(fixture, &producer, producer_verify)?;
        assert!(!derived.exists());
        let producer_ms = started.elapsed().as_millis();
        assert!(receipt.contains("origin-fetches=0\n"), "{receipt}");
        let before = fixture.metrics()?;
        fixture.deny_archives()?;
        let reader = Sandbox::new()?;
        restore_declared_paths(job, &producer, &reader)?;
        if let Some(path) = std::env::var_os("VELNOR_NONRUST_PAYLOAD_EXPORT") {
            copy_tree(
                &reader.mise_root(),
                &std::path::PathBuf::from(path).join(id),
            )?;
        }
        eprintln!(
            "saved_extracted_bytes={} saved_archive_bytes={}",
            super::nonrust_tools_fixture::tree_bytes(&producer.mise_root())?,
            super::nonrust_tools_fixture::tree_bytes(
                &producer
                    .mise_root()
                    .join("installs/.velnor-verified-archives")
            )?
        );
        self.consume(
            &reader,
            &recipe,
            &selected,
            ProducerReceipt {
                producer_ms,
                receipt,
                before,
            },
        )
    }

    fn consume(
        &self,
        reader: &Sandbox,
        recipe: &RenderedCommand,
        selected: &[&super::nonrust_tools_fixture::Archive],
        producer: ProducerReceipt,
    ) -> Result<std::collections::BTreeSet<String>, Box<dyn Error>> {
        let Self {
            fixture,
            reader_verify,
            ..
        } = self;
        let ProducerReceipt {
            producer_ms,
            receipt,
            before,
        } = producer;
        let mut observed = std::collections::BTreeSet::new();
        let started = std::time::Instant::now();
        let warm = execute(fixture, reader, reader_verify)?;
        assert!(
            warm.contains("origin-fetches=0\n") && warm.contains("origin-bytes=0\n"),
            "{warm}"
        );
        assert!(
            !warm.contains("reused-archives=0\n") && !warm.contains("verified-files=0\n"),
            "{warm}"
        );
        execute(fixture, reader, recipe)?;
        execute(fixture, reader, self.jactionlint_verify)?;
        for archive in selected {
            let saved = reader
                .mise_root()
                .join("installs/.velnor-verified-archives")
                .join(&archive.sha256);
            assert_eq!(fs::metadata(saved)?.len(), archive.bytes);
            assert!(archive.url.starts_with("https://"));
            observed.insert(archive.tool.clone());
            fixture.execute(reader, &archive.tool, &archive.version)?;
        }
        let after = fixture.metrics()?;
        assert_eq!(after.archive_requests, before.archive_requests);
        assert_eq!(after.misses, 0);
        eprintln!(
            "producer_ms={producer_ms} reader_ms={} producer_receipt={receipt:?} reader_receipt={warm:?} transport_before={} transport_after={}",
            started.elapsed().as_millis(),
            serde_json::to_string(&before)?,
            serde_json::to_string(&after)?
        );
        super::nonrust_tools_corruption::verify(fixture, reader, reader_verify, selected)?;
        Ok(observed)
    }
}

struct ProducerReceipt {
    producer_ms: u128,
    receipt: String,
    before: super::nonrust_tools_fixture::Metrics,
}
