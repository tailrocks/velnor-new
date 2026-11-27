// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::exec_support::Ran;
use crate::mise_world::{VERSION, World, job_fixture};

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

pub(crate) type Check = Box<dyn Fn(&World, &RealMise) -> Result<()>>;
pub(crate) type Verify = Box<dyn Fn(&World, &Ran) -> Option<String>>;

#[derive(Default)]
pub(crate) struct Row {
    pub(crate) id: &'static str,
    pub(crate) plants: Vec<Plant>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) control_cwd: Option<PathBuf>,
    pub(crate) hides_workspace_config: bool,
    pub(crate) config: Option<(String, String)>,
    pub(crate) prepare: Option<Check>,
    pub(crate) control: Option<Check>,
    pub(crate) prelude: Option<String>,
    pub(crate) script: Option<String>,
    pub(crate) verify: Option<Verify>,
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

fn version_of(binary: &Path, world: &World) -> Result<String> {
    let output = Command::new(binary)
        .arg("--version")
        .current_dir(world.home())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", world.home())
        .env("MISE_OFFLINE", "1")
        .output()
        .context("run mise --version")?;
    ensure!(output.status.success(), "mise --version failed");
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(crate) fn real_mise(world: &World) -> Result<RealMise> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::fs::create_dir_all(world.home()).context("create the home directory")?;
    let mut seen = Vec::new();
    for candidate in std::env::split_paths(&path)
        .map(|directory| directory.join("mise"))
        .filter(|candidate| is_executable(candidate))
    {
        let binary = std::fs::canonicalize(&candidate).context("resolve the real mise binary")?;
        let version = version_of(&binary, world)?;
        if version.split(' ').next() == Some(VERSION) {
            return Ok(RealMise { binary, version });
        }
        seen.push(format!("{} is {version}", binary.display()));
    }
    bail!(
        "{MISSING} at the version {VERSION} pinned in .velnor/version-policy.toml; found {seen:?}"
    )
}

impl World {
    pub(crate) fn install_real_mise(&self, real: &RealMise) -> Result<()> {
        self.unseal_store()?;
        let directory = self.binary_dir();
        std::fs::create_dir_all(&directory).context("create the tools store")?;
        std::fs::copy(&real.binary, directory.join("mise"))
            .map(drop)
            .context("copy the real mise binary")
    }

    pub(crate) fn ceiling(&self) -> PathBuf {
        canonical(
            self.root
                .parent()
                .map_or(self.root.as_path(), |parent| parent),
        )
    }

    pub(crate) fn start_clean(&self, job: &Path, extra_env: &[(&str, &str)]) -> Result<Child> {
        self.unseal_store()?;
        let mut command = self.command(job)?;
        self.seal_store()?;
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .env("VELNOR_STORE_ROOT", &self.store)
            .env("MISE_CEILING_PATHS", self.ceiling())
            .envs(extra_env.iter().copied());
        self.declare_read_only(&mut command);
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

pub(crate) fn canonical(path: &Path) -> PathBuf {
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
    use std::os::unix::fs::PermissionsExt;
    for plant in &row.plants {
        if let Some(parent) = plant.path.parent() {
            std::fs::create_dir_all(parent).context("create the plant directory")?;
        }
        if let Some(marker) = plant.marker.as_ref().and_then(|marker| marker.parent()) {
            std::fs::create_dir_all(marker).context("create the marker directory")?;
        }
        std::fs::write(&plant.path, &plant.content).context("write the plant")?;
        if plant.content.starts_with("#!") {
            std::fs::set_permissions(&plant.path, std::fs::Permissions::from_mode(0o755))
                .context("make the plant executable")?;
        }
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

fn prove_listing(world: &World, real: &RealMise, row: &Row) -> Result<()> {
    let cwd = row.control_cwd.clone().unwrap_or_else(|| world.workspace());
    std::fs::create_dir_all(&cwd).context("create the control directory")?;
    let listing: BTreeSet<PathBuf> = control_listing(world, real, &cwd, &row.env)?
        .into_iter()
        .collect();
    let mut expected: BTreeSet<PathBuf> = row
        .plants
        .iter()
        .filter(|plant| plant.listed)
        .map(|plant| canonical(&plant.path))
        .collect();
    if row.control_cwd.is_none() && !row.hides_workspace_config {
        expected.insert(canonical(&world.workspace().join("mise.toml")));
    }
    ensure!(
        listing == expected,
        "row {}: the control ({}) lists {listing:?}, expected exactly {expected:?}",
        row.id,
        real.version
    );
    Ok(())
}

fn prove_control(world: &World, real: &RealMise, row: &Row) -> Result<()> {
    match &row.control {
        Some(control) => control(world, real)?,
        None => prove_listing(world, real, row)?,
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
    let (config, lock) = row
        .config
        .as_ref()
        .map_or((ACCEPTED_CONFIG, ""), |(config, lock)| {
            (config.as_str(), lock.as_str())
        });
    world.init_workspace_with_lock(config, lock)?;
    write_plants(row)?;
    if let Some(prepare) = &row.prepare {
        prepare(world, real)?;
    }
    prove_control(world, real, row)?;
    let script = row
        .script
        .as_deref()
        .unwrap_or("echo \"accepted=$ACCEPTED\"");
    let mut job = world.with_final_script(&job_fixture("mise_action_ok"), script)?;
    if let Some(prelude) = &row.prelude {
        job = world.with_leading_script(&job, prelude)?;
    }
    let pairs: Vec<(&str, &str)> = row
        .env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let delivery = if pairs.is_empty() {
        "files"
    } else {
        "process env"
    };
    let mut failures = Vec::new();
    let ran = World::finish(world.start_clean(&job, &pairs)?)?;
    failures.extend(report(world, row, delivery, &ran, real));
    if !pairs.is_empty() {
        let with_env = world.with_job_env(&job, &pairs)?;
        let ran = World::finish(world.start_clean(&with_env, &[])?)?;
        failures.extend(report(world, row, "job message env", &ran, real));
    }
    Ok(failures)
}

fn report(world: &World, row: &Row, delivery: &str, ran: &Ran, real: &RealMise) -> Option<String> {
    let markers = fired(row);
    let accepted = ran.has_line("accepted=1");
    let succeeded = ran.has_line("job succeeded") && ran.code == Some(0);
    let verified = row.verify.as_ref().and_then(|verify| verify(world, ran));
    if markers.is_empty() && accepted && succeeded && verified.is_none() {
        return None;
    }
    let unsupported: Vec<&str> = ran
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("unsupported:"))
        .collect();
    Some(format!(
        "row {} [{delivery}] ({}): markers fired {markers:?}; accepted=1 seen {accepted}; job succeeded and exit 0 seen {succeeded}; {}{unsupported:?}",
        row.id,
        real.version,
        verified.map_or_default(|text| format!("{text}; "))
    ))
}
