// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use velnor_actions_contract::{Job, StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};

use super::{
    nonrust_tools_fixture::Fixture,
    nonrust_tools_roundtrip::{RenderedCommand, execute, restore_declared_paths, run_named},
    tools_cache_verify_hook::{Sandbox, expand_shell},
};

#[test]
fn actual_nonrust_producers_publish_qualified_subsets() -> Result<(), Box<dyn Error>> {
    let (fixture, _replay) = Fixture::load()?;
    for repository in [runtime_bun()?, runtime_families(&fixture)?] {
        verify_repository(&fixture, repository.path())?;
    }
    Ok(())
}

fn verify_repository(fixture: &Fixture, repository: &Path) -> Result<(), Box<dyn Error>> {
    let prepared = prepare(repository)?;
    let jobs = finalized_jobs(&prepared)?;
    let rendered = render_staged_tree(&prepared)?;
    let text = |path: &str| rendered.get(path).ok_or("missing generated file");
    let runtime = Runtime {
        fixture,
        repository,
        supplemental: repository.join("mise.lock").exists(),
        verify: run_named(
            text(".github/actions/velnor-tools-cache-verify/action.yml")?,
            "Re-verify restored Mise installs",
        )?,
        reader_verify: run_named(
            text(".github/actions/velnor-tools-cache-restore/action.yml")?,
            "Re-verify restored Mise installs",
        )?,
        dedicated: run_named(
            text(".github/actions/velnor-verify-jactionlint/action.yml")?,
            "Verify jactionlint",
        )?,
    };
    let seal: serde_json::Value =
        serde_saphyr::from_str(text(".github/workflows/state-seal.yml")?)?;
    let mut observed = 0;
    for (id, job) in &jobs {
        let Some(specs) = producer_specs(job)? else {
            continue;
        };
        eprintln!("candidate_profile={id} specs={specs:?}");
        if !(specs.contains("bun@") || runtime.supplemental && specs.contains("jactionlint@")) {
            continue;
        }
        let recipe = run_named(
            &serde_json::to_string(&seal["jobs"][id])?,
            "Prepare pinned tools",
        )?;
        runtime.produce(id, job, &recipe, &specs)?;
        observed += 1;
    }
    assert!(observed > 0);
    Ok(())
}

fn producer_specs(job: &Job) -> Result<Option<String>, Box<dyn Error>> {
    if !job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::ToolsCacheVerify))
    {
        return Ok(None);
    }
    let install = job
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::PreparePinnedTools))
        .ok_or("missing install")?;
    let StepKind::Shell { run, .. } = &install.kind else {
        return Err("non-shell install".into());
    };
    Ok(Some(run.join(" ")))
}

struct Runtime<'a> {
    fixture: &'a Fixture,
    repository: &'a Path,
    supplemental: bool,
    verify: RenderedCommand,
    reader_verify: RenderedCommand,
    dedicated: RenderedCommand,
}

impl Runtime<'_> {
    fn produce(
        &self,
        id: &str,
        job: &Job,
        recipe: &RenderedCommand,
        specs: &str,
    ) -> Result<(), Box<dyn Error>> {
        let producer = Sandbox::new()?;
        self.fixture.provision(&producer)?;
        copy_lock(self.repository, &producer)?;
        execute(self.fixture, &producer, recipe)?;
        if self.supplemental {
            for (tool, version) in [
                ("python", "3.14.9"),
                ("uv", "0.13.5"),
                ("dbt-fusion", "2.0.9"),
            ] {
                self.fixture.install(&producer, tool, version)?;
            }
        }
        self.fixture.install(&producer, "bun", "1.4.4")?;
        self.fixture.install(&producer, "jactionlint", "2.4.0")?;
        execute(self.fixture, &producer, &self.dedicated)?;
        let receipt = execute(self.fixture, &producer, &self.verify)?;
        if self.supplemental {
            self.assert_python_pruned(&producer)?;
        }
        assert!(!producer.mise_root().join("installs/bun/1.4.4").exists());
        assert!(
            producer
                .mise_root()
                .join("installs/jactionlint/2.4.0")
                .is_dir()
        );
        assert!(receipt.contains("origin-fetches=0\n"), "{receipt}");
        let reader = Sandbox::new()?;
        restore_declared_paths(job, &producer, &reader)?;
        copy_lock(self.repository, &reader)?;
        self.consume(&reader, recipe, specs)?;
        if self.supplemental {
            self.corrupt_locked_uv(&reader)?;
        }
        eprintln!("subset_profile={id} specs={specs:?} receipt={receipt:?}");
        Ok(())
    }

    fn consume(
        &self,
        reader: &Sandbox,
        recipe: &RenderedCommand,
        specs: &str,
    ) -> Result<(), Box<dyn Error>> {
        let before = self.fixture.metrics()?;
        execute(self.fixture, reader, &self.reader_verify)?;
        execute(self.fixture, reader, &self.dedicated)?;
        execute(self.fixture, reader, recipe)?;
        if self.supplemental {
            self.assert_python_pruned(reader)?;
            for (tool, version) in [("uv", "0.13.5"), ("dbt-fusion", "2.0.9")] {
                self.fixture.execute(reader, tool, version)?;
            }
        } else if specs.contains("bun@") {
            self.fixture.execute(reader, "bun", "1.4.4")?;
        }
        let warm = self.fixture.metrics()?;
        assert_eq!(warm.misses, 0);
        if self.supplemental {
            assert_eq!(warm.archive_requests, before.archive_requests);
        }
        eprintln!(
            "subset_warm_before={} subset_warm_after={}",
            serde_json::to_string(&before)?,
            serde_json::to_string(&warm)?
        );
        if self.supplemental {
            self.fixture.install(reader, "python", "3.14.9")?;
            self.fixture.execute(reader, "python", "3.14.9")?;
            let cold = self.fixture.metrics()?;
            assert_eq!(cold.misses, 0);
            eprintln!("subset_python_cold_after={}", serde_json::to_string(&cold)?);
        }
        Ok(())
    }

    fn assert_python_pruned(&self, runner: &Sandbox) -> Result<(), Box<dyn Error>> {
        assert!(!runner.mise_root().join("installs/python/3.14.9").exists());
        assert!(!self.archive(runner, "python")?.exists());
        Ok(())
    }

    fn archive(&self, runner: &Sandbox, tool: &str) -> Result<PathBuf, Box<dyn Error>> {
        let lock: toml::Value =
            toml::from_str(&fs::read_to_string(self.repository.join("mise.lock"))?)?;
        let platform = format!("platforms.{}", self.fixture.host);
        let checksum = lock
            .get("tools")
            .and_then(|tools| tools.get(tool))
            .and_then(toml::Value::as_array)
            .and_then(|tools| tools.first())
            .and_then(|tool| tool.get(platform.as_str()))
            .and_then(|platform| platform.get("checksum"))
            .and_then(toml::Value::as_str)
            .and_then(|checksum| checksum.strip_prefix("sha256:"))
            .ok_or("missing genuine platform checksum")?;
        Ok(runner
            .mise_root()
            .join("installs/.velnor-verified-archives")
            .join(checksum))
    }

    fn corrupt_locked_uv(&self, reader: &Sandbox) -> Result<(), Box<dyn Error>> {
        let install = reader.mise_root().join("installs/uv/0.13.5");
        let archive = self.archive(reader, "uv")?;
        assert!(archive.is_file());
        fs::write(uv_binary(&install)?, "corrupted genuine UV binary")?;
        let mut environment = self.fixture.environment(reader);
        environment.extend(
            self.verify
                .env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        let output = reader.run_with_output(
            &expand_shell(&self.verify.shell),
            &self.verify.run,
            &environment,
        )?;
        assert_eq!(output.code, Some(0), "{}\n{}", output.stdout, output.stderr);
        assert!(
            output.stdout.contains("discarding restored uv@0.13.5"),
            "{}",
            output.stdout
        );
        assert!(
            !output.stdout.contains("restored uv@0.13.5 matches"),
            "{}",
            output.stdout
        );
        assert!(!install.exists());
        assert!(!archive.exists());
        assert!(
            reader
                .mise_root()
                .join("installs/jactionlint/2.4.0")
                .is_dir()
        );
        assert!(output.github_output.contains("origin-fetches=0\n"));
        eprintln!(
            "lock_only_uv_corruption_discarded=true receipt={:?}",
            output.github_output
        );
        Ok(())
    }
}

fn uv_binary(install: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if install.join("uv").is_file() {
        return Ok(install.join("uv"));
    }
    for entry in fs::read_dir(install)? {
        let entry = entry?;
        let candidate = entry.path().join("uv");
        if entry.file_type()?.is_dir() && candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("genuine UV binary absent".into())
}

fn runtime_bun() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = super::bun_routing::make_bun_repo()?;
    fs::write(repo.path().join("mise.toml"), "[tools]\nbun = \"1.4.4\"\n")?;
    let manifest = repo.path().join("package.json");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)?.replace("bun@1.4.3", "bun@1.4.4"),
    )?;
    Ok(repo)
}

fn runtime_families(fixture: &Fixture) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = super::nonrust_family_fixture::repository()?;
    fs::remove_file(repo.path().join("Cargo.toml"))?;
    fs::remove_file(repo.path().join("src/lib.rs"))?;
    fs::write(
        repo.path().join("mise.toml"),
        "[tools]\ndbt-fusion = \"2.0.9\"\npython = \"3.14.9\"\nuv = \"0.13.5\"\nansible-core = \"2.19.0\"\n",
    )?;
    fs::copy(
        fixture
            .home
            .parent()
            .ok_or("fixture root absent")?
            .join("lock-project/mise.lock"),
        repo.path().join("mise.lock"),
    )?;
    Ok(repo)
}

fn copy_lock(repository: &Path, runner: &Sandbox) -> Result<(), Box<dyn Error>> {
    if repository.join("mise.lock").exists() {
        fs::copy(
            repository.join("mise.lock"),
            runner.workspace.path().join("mise.lock"),
        )?;
    }
    Ok(())
}
