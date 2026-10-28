// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{DiscoveredCheck, invalid};
use crate::{IsolatedCommand, MISE_CARGO_HOME_ENV, MISE_RUSTUP_HOME_ENV, MiseError};
use std::ffi::OsString;
use std::fmt::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct QualifiedCheck {
    pub(crate) home: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) cwd: PathBuf,
    pub(crate) check: DiscoveredCheck,
    pub(crate) capability_env: Vec<(OsString, OsString)>,
    pub(crate) mise_version: String,
    restored_overlay: Vec<(OsString, OsString)>,
}

impl QualifiedCheck {
    pub fn new(
        home: PathBuf,
        cwd: PathBuf,
        check: DiscoveredCheck,
        proof: super::CheckCapabilityProof,
        system_proofs: &[super::SystemToolProof],
        mise_version: String,
    ) -> Result<Self, MiseError> {
        if !home.is_absolute()
            || !cwd.is_absolute()
            || cwd.starts_with(&home)
            || home.starts_with(&cwd)
        {
            return Err(invalid("check_homes", "absolute_separate_paths_required"));
        }
        let config = home.join("tasks.toml");
        let mut capability_env = vec![];
        super::validate_system_tool_proofs(
            check.check.runner.platform,
            &check.check.system_tools,
            system_proofs,
        )?;
        if let Some(system) = system_proofs.first() {
            capability_env.push((
                OsString::from("DEVELOPER_DIR"),
                OsString::from(&system.developer_dir),
            ));
        }
        super::validate_check_capability_proof(&check.check.runner, &proof)?;
        if let Some(container) = proof.container {
            if !container.docker_program.starts_with(&home)
                || container
                    .orbctl
                    .as_ref()
                    .is_some_and(|orb| !orb.program.starts_with(&home))
            {
                return Err(invalid("container_program", "owned_home_required"));
            }
            capability_env.push((
                OsString::from("DOCKER_HOST"),
                OsString::from(container.endpoint),
            ));
            capability_env.push((
                OsString::from("DOCKER_CONFIG"),
                home.join("docker").into_os_string(),
            ));
            let context = match container.profile {
                velnor_actions_contract::config::HostContainerProfile::Docker {
                    context, ..
                }
                | velnor_actions_contract::config::HostContainerProfile::OrbStack {
                    context, ..
                } => context,
            };
            capability_env.push((OsString::from("DOCKER_CONTEXT"), OsString::from(context)));
        }
        let owned = Self {
            home,
            config,
            cwd,
            check,
            capability_env,
            mise_version,
            restored_overlay: Vec::new(),
        };
        owned.bound_projection()?;
        Ok(owned)
    }
    pub fn command(&self, deadline: crate::CheckDeadline) -> Result<IsolatedCommand, MiseError> {
        IsolatedCommand::qualified_check_run(self, deadline)?.with_env(&self.restored_overlay)
    }

    pub fn project_restored_homes(
        &mut self,
        state_root: &Path,
        cargo_home: &Path,
        rustup_home: &Path,
        tool_root: &Path,
    ) -> Result<(), MiseError> {
        self.restored_overlay =
            self.restored_home_overlay(state_root, cargo_home, rustup_home, tool_root)?;
        Ok(())
    }

    #[must_use]
    pub fn restored_overlay(&self) -> &[(OsString, OsString)] {
        &self.restored_overlay
    }
    #[must_use]
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(u64::from(self.check.check.timeout_minutes) * 60)
    }

    #[must_use]
    pub fn mise_program(&self) -> PathBuf {
        self.home.join("bin/mise")
    }
}

impl QualifiedCheck {
    pub fn bound_projection(&self) -> Result<String, MiseError> {
        let dir = toml_quote(
            self.cwd
                .to_str()
                .ok_or_else(|| invalid("check_directory", "invalid_utf8"))?,
        );
        let mut out = String::new();
        let doc = crate::toml_scan::parse_toml(&self.check.task_config)
            .map_err(|e| invalid("check_projection", format!("{}:{}", e.line, e.problem)))?;
        let headers: std::collections::BTreeSet<usize> = doc
            .sections
            .iter()
            .map(|(_, line)| *line as usize)
            .collect();
        let line_count = self.check.task_config.lines().count();
        let mut boundaries = headers.clone();
        boundaries.extend(doc.assignments.iter().map(|a| a.line as usize));
        boundaries.insert(line_count + 1);
        let mut omitted = std::collections::BTreeSet::new();
        for assignment in &doc.assignments {
            if assignment
                .path
                .last()
                .is_some_and(|p| p == "sources" || p == "outputs")
            {
                let start = assignment.line as usize;
                if let Some(end) = boundaries.range(start + 1..).next() {
                    omitted.extend(start..*end);
                }
            }
        }
        for (index, line) in self.check.task_config.split_inclusive('\n').enumerate() {
            if omitted.contains(&(index + 1)) {
                continue;
            }
            out.push_str(line);
            if headers.contains(&(index + 1)) {
                writeln!(out, "dir = {dir}")
                    .map_err(|error| invalid("check_projection", error.to_string()))?;
            }
        }
        Ok(out)
    }
    pub(crate) fn owned_env(&self) -> Result<Vec<(OsString, OsString)>, MiseError> {
        let values = [
            ("HOME", self.home.clone()),
            ("XDG_CONFIG_HOME", self.home.join("config")),
            ("XDG_DATA_HOME", self.home.join("data")),
            ("XDG_CACHE_HOME", self.home.join("cache")),
            ("MISE_DATA_DIR", self.home.join("data")),
            ("MISE_CACHE_DIR", self.home.join("cache")),
            ("MISE_STATE_DIR", self.home.join("state")),
            ("MISE_GLOBAL_CONFIG_FILE", self.home.join("empty.toml")),
            ("MISE_OVERRIDE_CONFIG_FILENAMES", self.config.clone()),
            ("MISE_TRUSTED_CONFIG_PATHS", self.config.clone()),
            ("CARGO_HOME", self.home.join("cargo")),
            (
                "RUSTUP_HOME",
                self.home.join(crate::checks::RUSTUP_HOME_SUFFIX),
            ),
            ("MISE_CARGO_HOME", self.home.join("cargo")),
            (
                "MISE_RUSTUP_HOME",
                self.home.join(crate::checks::RUSTUP_HOME_SUFFIX),
            ),
        ];
        let mut env: Vec<_> = values
            .into_iter()
            .map(|(k, v)| (OsString::from(k), v.into_os_string()))
            .collect();
        let mut paths: Vec<PathBuf> = vec![self.home.join("bin")];
        paths.extend(
            ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
                .iter()
                .map(PathBuf::from),
        );
        env.push((
            OsString::from("PATH"),
            std::env::join_paths(paths).map_err(|e| invalid("check_path", e.to_string()))?,
        ));
        env.push((
            OsString::from("MISE_TASK_RUN_AUTO_INSTALL"),
            OsString::from("false"),
        ));
        env.push((OsString::from("MISE_TASK_CACHE"), OsString::from("off")));
        env.extend(
            [
                ("MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES", "none"),
                ("MISE_IDIOMATIC_VERSION_FILE_ENABLE_TOOLS", ""),
            ]
            .map(|(key, value)| (OsString::from(key), OsString::from(value))),
        );
        if let Some(prefix) = self.compiler_prefix()? {
            env.push((OsString::from("RUSTUP_TOOLCHAIN"), prefix.into_os_string()));
        }
        env.extend(self.capability_env.clone());
        Ok(env)
    }

    pub fn restored_home_overlay(
        &self,
        state_root: &Path,
        cargo_home: &Path,
        rustup_home: &Path,
        tool_root: &Path,
    ) -> Result<Vec<(OsString, OsString)>, MiseError> {
        for (field, root) in [
            ("execution_state_root", state_root),
            ("execution_cargo_home", cargo_home),
            ("execution_rustup_home", rustup_home),
            ("execution_tool_root", tool_root),
        ] {
            if !root.is_absolute() {
                return Err(invalid(field, "absolute_owned_root_required"));
            }
            if root == self.home
                || root == self.cwd
                || root.starts_with(&self.home)
                || self.home.starts_with(root)
                || root.starts_with(&self.cwd)
                || self.cwd.starts_with(root)
            {
                return Err(invalid(field, "shared_home_overlaps_private_home"));
            }
        }
        let roots = [state_root, cargo_home, rustup_home, tool_root];
        for (index, first) in roots.iter().enumerate() {
            for second in &roots[index + 1..] {
                if first == second || first.starts_with(second) || second.starts_with(first) {
                    return Err(invalid("execution_roots", "shared_roots_must_be_distinct"));
                }
            }
        }
        Ok(vec![
            (
                OsString::from("CARGO_HOME"),
                cargo_home.as_os_str().to_owned(),
            ),
            (
                OsString::from("RUSTUP_HOME"),
                rustup_home.as_os_str().to_owned(),
            ),
            (
                OsString::from(MISE_CARGO_HOME_ENV),
                cargo_home.as_os_str().to_owned(),
            ),
            (
                OsString::from(MISE_RUSTUP_HOME_ENV),
                rustup_home.as_os_str().to_owned(),
            ),
        ])
    }

    pub fn lane_target_dir(&self, state_root: &Path, lane: &str) -> Result<PathBuf, MiseError> {
        let dir = lane_dir(state_root, lane, "target")?;
        reject_private_overlap(&self.home, &self.cwd, &dir)?;
        Ok(dir)
    }

    pub fn lane_build_dir(&self, state_root: &Path, lane: &str) -> Result<PathBuf, MiseError> {
        let dir = lane_dir(state_root, lane, "build")?;
        reject_private_overlap(&self.home, &self.cwd, &dir)?;
        Ok(dir)
    }

    fn compiler_prefix(&self) -> Result<Option<PathBuf>, MiseError> {
        use velnor_actions_contract::config::QualifiedToolBackend;
        let mut compilers = self.check.qualified_tools.iter().filter(
            |tool| matches!(&tool.backend, QualifiedToolBackend::Core { tool } if tool == "rust"),
        );
        let prefix = compilers
            .next()
            .map(|tool| self.home.join("tools").join(&tool.id).join("prefix"));
        if compilers.next().is_some() {
            return Err(invalid("check_compiler", "multiple_rust_installations"));
        }
        Ok(prefix)
    }
}

fn lane_dir(state_root: &Path, lane: &str, kind: &str) -> Result<PathBuf, MiseError> {
    if !state_root.is_absolute() {
        return Err(invalid(
            "execution_state_root",
            "absolute_owned_root_required",
        ));
    }
    if lane.is_empty()
        || lane == "."
        || lane == ".."
        || lane.contains('/')
        || lane.contains('\\')
        || lane.contains('\0')
    {
        return Err(invalid("execution_lane", "invalid_lane_id"));
    }
    Ok(state_root.join(lane).join(kind))
}

fn reject_private_overlap(home: &Path, cwd: &Path, dir: &Path) -> Result<(), MiseError> {
    if dir == home
        || dir == cwd
        || dir.starts_with(home)
        || dir.starts_with(cwd)
        || home.starts_with(dir)
        || cwd.starts_with(dir)
    {
        return Err(invalid(
            "execution_lane_dir",
            "lane_dir_overlaps_private_home",
        ));
    }
    Ok(())
}

fn toml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}
