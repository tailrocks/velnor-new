// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::repository_orchestrator_repo;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use velnor_actions_contract::StepRole;
use velnor_actions_orchestrator::{
    GenerationPreparation, finalized_jobs, prepare, render_staged_tree,
};

#[test]
fn root_bun_pin_and_lock_changes_change_the_emitted_tools_cache_key() -> Result<(), Box<dyn Error>>
{
    let resolver = ProjectToolResolver::new()?;
    let baseline = repository_orchestrator_repo(false)?;
    let baseline_prepared = prepare(baseline.path())?;
    let baseline_key = emitted_tools_cache_key(&baseline_prepared, baseline.path(), &resolver)?;

    let version_change = repository_orchestrator_repo(false)?;
    change_bun_version(version_change.path(), "1.4.3")?;
    let version_prepared = prepare(version_change.path())?;
    let version_key = emitted_tools_cache_key(&version_prepared, version_change.path(), &resolver)?;
    assert_ne!(
        baseline_key, version_key,
        "Bun version changes must change the cache key"
    );

    let lock_change = repository_orchestrator_repo(false)?;
    change_bun_lock_checksum(lock_change.path())?;
    let lock_prepared = prepare(lock_change.path())?;
    let lock_key = emitted_tools_cache_key(&lock_prepared, lock_change.path(), &resolver)?;
    assert_ne!(
        baseline_key, lock_key,
        "Bun lock changes must change the cache key"
    );
    Ok(())
}

struct ProjectToolResolver {
    _temp: tempfile::TempDir,
    home: PathBuf,
    mise_data: PathBuf,
    runner_temp: PathBuf,
    path: std::ffi::OsString,
}

impl ProjectToolResolver {
    fn new() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let home = root.join("home");
        let mise_data = root.join("mise-data");
        let runner_temp = root.join("runner-temp");
        fs::create_dir_all(home.join(".local/share/mise"))?;
        fs::create_dir_all(mise_data.join("bin"))?;
        fs::create_dir_all(&runner_temp)?;
        let path = env::var_os("PATH").ok_or("PATH is missing")?;
        let mise = env::split_paths(&path)
            .map(|entry| entry.join("mise"))
            .find(|candidate| candidate.is_file())
            .ok_or("the selected Mise executable is missing")?;
        fs::copy(mise, mise_data.join("bin/mise"))?;
        Ok(Self {
            _temp: temp,
            home,
            mise_data,
            runner_temp,
            path,
        })
    }

    fn resolve(
        &self,
        root: &Path,
        names: &BTreeSet<String>,
        suffix: &str,
    ) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
        let github_env = self.runner_temp.join(format!("github-env-{suffix}"));
        fs::write(&github_env, "")?;
        let script =
            velnor_actions_contract::config::project_tool_resolver::project_tool_resolver_script(
                names, false,
            );
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg(script)
            .current_dir(root)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("MISE_DATA_DIR", &self.mise_data)
            .env("MISE_CACHE_DIR", self.runner_temp.join("mise-cache"))
            .env("MISE_STATE_DIR", self.runner_temp.join("mise-state"))
            .env("MISE_CONFIG_DIR", self.runner_temp.join("mise-config"))
            .env("RUNNER_TEMP", &self.runner_temp)
            .env("RUNNER_OS", "Linux")
            .env("RUNNER_ARCH", "X64")
            .env("GITHUB_ENV", &github_env)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "emitted project tool resolver failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let values = fs::read_to_string(github_env)?
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect::<BTreeMap<_, _>>();
        Ok(values)
    }
}

fn emitted_tools_cache_key(
    prepared: &GenerationPreparation,
    root: &Path,
    resolver: &ProjectToolResolver,
) -> Result<String, Box<dyn Error>> {
    use velnor_actions_contract::StepKind;

    let jobs = finalized_jobs(prepared)?;
    let job = jobs
        .values()
        .find(|job| job.display_name == "Rust / velnor-actions-orchestrator")
        .ok_or("Orchestrator task job is missing")?;
    let template = job
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::ToolsCacheIdentity))
        .and_then(|step| match &step.kind {
            StepKind::Action { with, .. } => with.values().next(),
            _ => None,
        })
        .ok_or("generated tools cache identity input is missing")?;
    let names =
        velnor_actions_contract::config::project_tool_refs::project_tool_ref_names(template)
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
    assert!(names.contains("VELNOR_TOOL_BUN"));
    assert!(names.contains("VELNOR_TOOL_BUN_LOCK"));
    let values = resolver.resolve(root, &names, "cache-key")?;
    let tool_refs = velnor_actions_contract::config::project_tool_refs::map_project_tool_refs(
        template,
        |name| values.get(name).cloned().unwrap_or_default(),
    );

    let tree = render_staged_tree(prepared)?;
    let script = tree
        .get(".github/scripts/velnor-tools-cache-identity.sh")
        .ok_or("generated tools cache identity script is missing")?;
    run_tools_identity_script(script, &tool_refs)
}

fn run_tools_identity_script(script: &str, static_digest: &str) -> Result<String, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let home = root.join("home");
    let runner_temp = root.join("runner-temp");
    fs::create_dir_all(home.join(".local/share/mise"))?;
    fs::create_dir_all(&runner_temp)?;
    let script_path = root.join("tools-cache-identity.sh");
    let output_path = root.join("github-output");
    fs::write(&script_path, script)?;
    fs::write(&output_path, "")?;
    let path = env::var_os("PATH").ok_or("PATH is missing")?;
    let output = Command::new("/bin/bash")
        .arg(&script_path)
        .env_clear()
        .env("PATH", path)
        .env("HOME", &home)
        .env("RUNNER_TEMP", &runner_temp)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("ImageOS", "ubuntu26")
        .env("VELNOR_CACHE_LANE", "ubuntu-26.04")
        .env("VELNOR_CACHE_STATIC_DIGEST", static_digest)
        .env("MISE_DATA_DIR", home.join(".local/share/mise"))
        .env("MISE_RUSTUP_HOME", runner_temp.join("velnor/rustup"))
        .env("RUSTUP_HOME", runner_temp.join("velnor/rustup"))
        .env("MISE_CARGO_HOME", runner_temp.join("velnor/cargo"))
        .env("CARGO_HOME", runner_temp.join("velnor/cargo"))
        .env("GITHUB_OUTPUT", &output_path)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "emitted tools cache identity failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let output = fs::read_to_string(output_path)?;
    let value = output
        .lines()
        .find_map(|line| line.strip_prefix("identity="))
        .ok_or("emitted cache identity did not write its fingerprint")?;
    Ok(format!("mise-tools-v2-{value}"))
}

fn change_bun_version(root: &Path, version: &str) -> Result<(), Box<dyn Error>> {
    let config_path = root.join("mise.toml");
    let config = fs::read_to_string(&config_path)?;
    let updated_config = config.replace("bun = \"1.4.4\"", &format!("bun = \"{version}\""));
    if updated_config == config {
        return Err("project mise.toml Bun pin was not found".into());
    }
    fs::write(config_path, updated_config)?;

    let lock_path = root.join("mise.lock");
    let mut lock = fs::read_to_string(&lock_path)?;
    let (start, end) = bun_lock_span(&lock)?;
    let section = lock[start..end].replace("1.4.4", version);
    if section == lock[start..end] {
        return Err("project mise.lock Bun version was not found".into());
    }
    lock.replace_range(start..end, &section);
    fs::write(lock_path, lock)?;
    Ok(())
}

fn change_bun_lock_checksum(root: &Path) -> Result<(), Box<dyn Error>> {
    let path = root.join("mise.lock");
    let mut lock = fs::read_to_string(&path)?;
    let (start, end) = bun_lock_span(&lock)?;
    let section = &lock[start..end];
    let platform = section
        .find("[tools.bun.\"platforms.linux-x64\"]")
        .ok_or("Linux Bun lock row is missing")?;
    let row = &section[platform..];
    let checksum = row
        .find("checksum = \"sha256:")
        .ok_or("Linux Bun lock checksum is missing")?
        + "checksum = \"sha256:".len();
    let position = start + platform + checksum;
    let replacement = if lock.as_bytes()[position] == b'a' {
        'b'
    } else {
        'a'
    };
    lock.replace_range(position..=position, &replacement.to_string());
    fs::write(path, lock)?;
    Ok(())
}

fn bun_lock_span(lock: &str) -> Result<(usize, usize), Box<dyn Error>> {
    let start = lock
        .find("[[tools.bun]]")
        .ok_or("Bun lock table is missing")?;
    let tail = &lock[start + "[[tools.bun]]".len()..];
    let end = tail
        .find("\n[[tools.")
        .map_or(lock.len(), |offset| start + "[[tools.bun]]".len() + offset);
    Ok((start, end))
}
