// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::exec_support::Ran;
use crate::mise_world::{World, job_fixture};

pub(crate) const ACCEPTED_CONFIG: &str = "[settings]\nlockfile = true\n\n[env]\nACCEPTED = \"1\"\n";
const MISSING: &str = "real mise binary required on PATH";

pub(crate) struct RealMise {
    pub(crate) binary: PathBuf,
    pub(crate) version: String,
}

pub(crate) struct Plant {
    pub(crate) path: PathBuf,
    pub(crate) content: String,
    pub(crate) marker: Option<PathBuf>,
    pub(crate) listed: bool,
}

pub(crate) struct Row {
    pub(crate) id: &'static str,
    pub(crate) plants: Vec<Plant>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) control_cwd: Option<PathBuf>,
}

impl Plant {
    pub(crate) fn exec(path: PathBuf, marker: PathBuf) -> Self {
        let content = format!(
            "[vars]\nV = \"{{{{ exec(command='touch {}') }}}}\"\n",
            marker.display()
        );
        Self {
            path,
            content,
            marker: Some(marker),
            listed: true,
        }
    }

    pub(crate) fn plain(path: PathBuf, content: &str, listed: bool) -> Self {
        Self {
            path,
            content: content.to_owned(),
            marker: None,
            listed,
        }
    }
}

#[derive(Deserialize)]
struct Listed {
    path: PathBuf,
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

pub(crate) fn real_mise(world: &World) -> Result<RealMise> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(found) = std::env::split_paths(&path)
        .map(|directory| directory.join("mise"))
        .find(|candidate| is_executable(candidate))
    else {
        bail!(MISSING);
    };
    let binary = std::fs::canonicalize(&found).context("resolve the real mise binary")?;
    std::fs::create_dir_all(world.home()).context("create the home directory")?;
    let output = Command::new(&binary)
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", world.home())
        .env("MISE_OFFLINE", "1")
        .output()
        .context("run mise --version")?;
    ensure!(output.status.success(), "mise --version failed");
    Ok(RealMise {
        binary,
        version: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    })
}

impl World {
    pub(crate) fn install_real_mise(&self, real: &RealMise) -> Result<()> {
        let directory = self.binary_dir();
        std::fs::create_dir_all(&directory).context("create the tools store")?;
        std::os::unix::fs::symlink(&real.binary, directory.join("mise"))
            .context("link the real mise binary")
    }

    pub(crate) fn ceiling(&self) -> PathBuf {
        self.root
            .parent()
            .map_or_else(|| self.root.clone(), Path::to_path_buf)
    }

    pub(crate) fn start_clean(&self, job: &Path, extra_env: &[(&str, &str)]) -> Result<Child> {
        let mut command = self.command(job);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .env("VELNOR_STORE_ROOT", &self.store)
            .env("MISE_CEILING_PATHS", self.ceiling())
            .envs(extra_env.iter().copied());
        command.spawn().context("start velnor-runner exec")
    }

    pub(crate) fn control(
        &self,
        real: &RealMise,
        cwd: &Path,
        row_env: &[(String, String)],
    ) -> Command {
        let empty = self.root.join("empty-system");
        let mut command = Command::new(&real.binary);
        command
            .current_dir(cwd)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.binary_dir().display()),
            )
            .env("HOME", self.home())
            .env("MISE_OFFLINE", "1")
            .env("MISE_YES", "1")
            .env("MISE_TRUSTED_CONFIG_PATHS", &self.root)
            .env("MISE_DATA_DIR", self.root.join("control-data"))
            .env("MISE_CACHE_DIR", self.root.join("control-cache"))
            .env("MISE_SYSTEM_CONFIG_DIR", &empty)
            .env("MISE_CEILING_PATHS", self.ceiling())
            .envs(row_env.iter().map(|(name, value)| (name, value)));
        command
    }
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn control_listing(
    world: &World,
    real: &RealMise,
    cwd: &Path,
    env: &[(String, String)],
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(world.root.join("empty-system")).context("create the empty dir")?;
    let output = world
        .control(real, cwd, env)
        .args(["config", "ls", "--json"])
        .output()
        .context("run the control")?;
    ensure!(
        output.status.success(),
        "control config ls failed ({}): {}",
        real.version,
        String::from_utf8_lossy(&output.stderr)
    );
    let listed: Vec<Listed> =
        serde_json::from_slice(&output.stdout).context("parse the listing")?;
    Ok(listed.iter().map(|item| canonical(&item.path)).collect())
}

fn write_plants(row: &Row) -> Result<()> {
    for plant in &row.plants {
        if let Some(parent) = plant.path.parent() {
            std::fs::create_dir_all(parent).context("create the plant directory")?;
        }
        if let Some(marker) = plant.marker.as_ref().and_then(|marker| marker.parent()) {
            std::fs::create_dir_all(marker).context("create the marker directory")?;
        }
        std::fs::write(&plant.path, &plant.content).context("write the plant")?;
    }
    Ok(())
}

fn fired(row: &Row) -> Vec<String> {
    let mut names = Vec::new();
    for plant in &row.plants {
        if let Some(marker) = plant.marker.as_ref().filter(|marker| marker.exists()) {
            names.push(
                marker
                    .file_name()
                    .map_or_default(|name| name.to_string_lossy().into_owned()),
            );
            let _ = std::fs::remove_file(marker);
        }
    }
    names
}

fn prove_control(world: &World, real: &RealMise, row: &Row) -> Result<()> {
    let cwd = row.control_cwd.clone().unwrap_or_else(|| world.workspace());
    std::fs::create_dir_all(&cwd).context("create the control directory")?;
    let listing = control_listing(world, real, &cwd, &row.env)?;
    for plant in row.plants.iter().filter(|plant| plant.listed) {
        let expected = canonical(&plant.path);
        ensure!(
            listing.contains(&expected),
            "row {}: the control ({}) does not list {}: {listing:?}",
            row.id,
            real.version,
            expected.display()
        );
    }
    let unfired: Vec<String> = row
        .plants
        .iter()
        .filter_map(|plant| plant.marker.as_ref().filter(|marker| !marker.exists()))
        .map(|marker| marker.display().to_string())
        .collect();
    ensure!(
        unfired.is_empty(),
        "row {}: the control ({}) did not run {unfired:?}",
        row.id,
        real.version
    );
    fired(row);
    Ok(())
}

pub(crate) fn attack(world: &World, real: &RealMise, row: &Row) -> Result<Vec<String>> {
    world.install_real_mise(real)?;
    world.init_workspace_with_lock(ACCEPTED_CONFIG, "")?;
    write_plants(row)?;
    prove_control(world, real, row)?;
    let job = world.with_final_script(
        &job_fixture("mise_action_ok"),
        "echo \"accepted=$ACCEPTED\"",
    )?;
    let pairs: Vec<(&str, &str)> = row
        .env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let mut runs: Vec<(&str, Child)> = Vec::new();
    if pairs.is_empty() {
        runs.push(("files", world.start_clean(&job, &[])?));
    } else {
        runs.push(("process env", world.start_clean(&job, &pairs)?));
    }
    let mut failures = Vec::new();
    for (delivery, child) in runs {
        failures.extend(report(row, delivery, &World::finish(child)?, real));
    }
    if !pairs.is_empty() {
        let with_env = world.with_job_env(&job, &pairs)?;
        let ran = World::finish(world.start_clean(&with_env, &[])?)?;
        failures.extend(report(row, "job message env", &ran, real));
    }
    Ok(failures)
}

fn report(row: &Row, delivery: &str, ran: &Ran, real: &RealMise) -> Option<String> {
    let markers = fired(row);
    let accepted = ran.has_line("accepted=1");
    let succeeded = ran.has_line("job succeeded") && ran.code == Some(0);
    if markers.is_empty() && accepted && succeeded {
        return None;
    }
    let unsupported: Vec<&str> = ran
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("unsupported:"))
        .collect();
    Some(format!(
        "row {} [{delivery}] ({}): markers fired {markers:?}; accepted=1 seen {accepted}; job succeeded and exit 0 seen {succeeded}; {unsupported:?}",
        row.id, real.version
    ))
}
