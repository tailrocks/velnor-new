use super::{DiscoveredCheck, invalid};
use crate::{IsolatedCommand, MiseError};
use std::ffi::OsString;
use std::fmt::Write;
use std::path::PathBuf;

/// Pure command handle for externally owned isolated execution state.
#[derive(Debug)]
pub struct QualifiedCheck {
    pub(crate) home: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) cwd: PathBuf,
    pub(crate) check: DiscoveredCheck,
    pub(crate) capability_env: Vec<(OsString, OsString)>,
}

impl QualifiedCheck {
    /// Bind a validated task to externally prepared, owned temporary homes.
    /// The orchestrator owns files, executable projections, and cleanup.
    /// # Errors
    /// Rejects relative homes/cwd or malformed task projection.
    pub fn new(
        home: PathBuf,
        cwd: PathBuf,
        check: DiscoveredCheck,
        proof: super::CheckCapabilityProof,
        system_proofs: &[super::SystemToolProof],
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
        };
        owned.bound_projection()?;
        Ok(owned)
    }
    /// Config-visible execution of the authorized named native task.
    /// # Errors
    /// Fails if the owned projection has changed.
    pub fn command(&self, deadline: crate::CheckDeadline) -> Result<IsolatedCommand, MiseError> {
        IsolatedCommand::qualified_check_run(self, deadline)
    }
    /// Per-check process deadline (includes the native task graph).
    #[must_use]
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(u64::from(self.check.check.timeout_minutes) * 60)
    }

    /// Exact owned executable materialized and verified by the orchestrator.
    #[must_use]
    pub fn mise_program(&self) -> PathBuf {
        self.home.join("bin/mise")
    }
}

impl QualifiedCheck {
    /// Task-only native config with fixed cwd and disabled freshness.
    /// # Errors
    /// Rejects malformed task bytes and unsupported directory encoding.
    pub fn bound_projection(&self) -> Result<String, MiseError> {
        // Static discovery rejects per-task dir and nested task tables. Binding
        // each copied native table preserves its original repository cwd.
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

fn toml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}
