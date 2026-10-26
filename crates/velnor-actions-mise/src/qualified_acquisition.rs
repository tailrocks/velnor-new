// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{EnvPolicy, IsolatedCommand, MISE_GLOBAL_FLAGS};
use crate::MiseError;
use crate::checks::{QualifiedCheck, invalid};
use std::ffi::OsString;
use std::path::PathBuf;
use velnor_actions_contract::config::{
    QualifiedCargoInstallation, QualifiedTool, QualifiedToolBackend, QualifiedToolOptions,
};

impl QualifiedCheck {
    pub fn qualified_tool_home(&self, id: &str) -> Result<PathBuf, MiseError> {
        self.selected_tool(id)?;
        Ok(self.home.join("tools").join(id))
    }

    pub fn qualified_source_command(&self, id: &str) -> Result<IsolatedCommand, MiseError> {
        let tool = self.selected_tool(id)?;
        let QualifiedToolOptions::Cargo {
            default_features,
            features,
            installation: QualifiedCargoInstallation::Source { .. },
        } = &tool.options
        else {
            return Err(invalid("qualified_acquisition", "cargo_source_required"));
        };
        if !matches!(tool.backend, QualifiedToolBackend::Cargo { .. }) {
            return Err(invalid("qualified_acquisition", "cargo_backend_required"));
        }
        let rust = self.rust_prerequisite(tool)?;
        let cargo = self.tool_executable(rust, "cargo")?;
        let rustc = self.tool_executable(rust, "rustc")?;
        let root = self.qualified_tool_home(id)?;
        let mut args: Vec<OsString> = ["install", "--locked", "--offline", "--no-track", "--path"]
            .into_iter()
            .map(OsString::from)
            .collect();
        args.push(root.join("sources").into_os_string());
        args.push(OsString::from("--root"));
        args.push(root.join("prefix").into_os_string());
        args.push(OsString::from("--config"));
        args.push(root.join("cargo-source.toml").into_os_string());
        if !default_features {
            args.push(OsString::from("--no-default-features"));
        }
        if !features.is_empty() {
            args.push(OsString::from("--features"));
            args.push(OsString::from(features.join(",")));
        }
        let mut command = self.acquisition_command(cargo.into_os_string(), args)?;
        command
            .extra_env
            .retain(|(key, _)| key != "CARGO_HOME" && key != "MISE_CARGO_HOME");
        command
            .extra_env
            .extend(self.source_environment(rust, &root)?);
        command
            .extra_env
            .push((OsString::from("RUSTC"), rustc.into_os_string()));
        command
            .extra_env
            .push((OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")));
        command.extra_env.push((
            OsString::from("CARGO_TARGET_DIR"),
            root.join("target").into_os_string(),
        ));
        command.cwd = Some(root);
        Ok(command)
    }

    pub fn qualified_link_command(&self, id: &str) -> Result<IsolatedCommand, MiseError> {
        let tool = self.selected_tool(id)?;
        let selector = match &tool.backend {
            QualifiedToolBackend::Core { tool } => format!("core:{tool}"),
            QualifiedToolBackend::Aqua { package } => format!("aqua:{package}"),
            QualifiedToolBackend::Cargo { crate_name } => format!("cargo:{crate_name}"),
        };
        let mut args: Vec<OsString> = MISE_GLOBAL_FLAGS.into_iter().map(OsString::from).collect();
        args.push(OsString::from("link"));
        args.push(OsString::from(format!("{selector}@{}", tool.version)));
        args.push(
            self.qualified_tool_home(id)?
                .join("prefix")
                .into_os_string(),
        );
        self.acquisition_command(self.mise_program().into_os_string(), args)
    }

    pub fn qualified_fetch_command(
        &self,
        id: &str,
        dependency: bool,
        index: usize,
    ) -> Result<IsolatedCommand, MiseError> {
        let tool = self.selected_tool(id)?;
        let platform = tool
            .platforms
            .iter()
            .find(|p| p.platform == self.check.check.runner.platform)
            .ok_or_else(|| invalid("qualified_acquisition", "missing_platform"))?;
        let artifacts = if dependency {
            &platform.dependency_artifacts
        } else {
            &platform.artifacts
        };
        let artifact = artifacts
            .get(index)
            .ok_or_else(|| invalid("qualified_acquisition", "missing_artifact"))?;
        let mut args: Vec<OsString> = [
            "--disable",
            "--globoff",
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
            "1073741824",
            "--output",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        args.push(
            self.qualified_tool_home(id)?
                .join("downloads")
                .join(&artifact.sha256)
                .into_os_string(),
        );
        args.push(OsString::from(&artifact.url));
        self.acquisition_command(OsString::from("/usr/bin/curl"), args)
    }

    fn selected_tool(&self, id: &str) -> Result<&QualifiedTool, MiseError> {
        let tool = self
            .check
            .qualified_tools
            .iter()
            .find(|tool| tool.id == id)
            .ok_or_else(|| invalid("qualified_acquisition", "unselected_tool"))?;
        tool.validate("qualified_tools", id)
            .map_err(|e| invalid("qualified_acquisition", e.to_string()))?;
        Ok(tool)
    }

    fn source_environment(
        &self,
        rust: &QualifiedTool,
        root: &std::path::Path,
    ) -> Result<Vec<(OsString, OsString)>, MiseError> {
        let mut env = vec![
            (
                OsString::from("CARGO_HOME"),
                root.join("cargo").into_os_string(),
            ),
            (
                OsString::from("MISE_CARGO_HOME"),
                root.join("cargo").into_os_string(),
            ),
            (
                OsString::from("RUSTUP_TOOLCHAIN"),
                self.qualified_tool_home(&rust.id)?
                    .join("prefix")
                    .into_os_string(),
            ),
        ];
        if rust.platforms.iter().any(|p| {
            p.platform == self.check.check.runner.platform
                && p.executables.iter().any(|e| e.name == "rustdoc")
        }) {
            env.push((
                OsString::from("RUSTDOC"),
                self.tool_executable(rust, "rustdoc")?.into_os_string(),
            ));
        }
        Ok(env)
    }

    fn rust_prerequisite(&self, tool: &QualifiedTool) -> Result<&QualifiedTool, MiseError> {
        for id in &tool.depends_on {
            let prerequisite = self.selected_tool(id)?;
            if matches!(&prerequisite.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
            {
                return Ok(prerequisite);
            }
        }
        Err(invalid(
            "qualified_acquisition",
            "missing_rust_prerequisite",
        ))
    }

    fn tool_executable(&self, tool: &QualifiedTool, name: &str) -> Result<PathBuf, MiseError> {
        let platform = tool
            .platforms
            .iter()
            .find(|p| p.platform == self.check.check.runner.platform)
            .ok_or_else(|| invalid("qualified_acquisition", "missing_platform"))?;
        let executable = platform
            .executables
            .iter()
            .find(|e| e.name == name)
            .ok_or_else(|| {
                invalid(
                    "qualified_acquisition",
                    format!("missing_executable:{name}"),
                )
            })?;
        Ok(self
            .qualified_tool_home(&tool.id)?
            .join("prefix")
            .join(&executable.path))
    }

    fn acquisition_command(
        &self,
        program: OsString,
        args: Vec<OsString>,
    ) -> Result<IsolatedCommand, MiseError> {
        let mut extra_env = self.owned_env()?;
        extra_env.retain(|(key, _)| {
            !matches!(
                key.to_str(),
                Some("DOCKER_HOST" | "DOCKER_CONFIG" | "DOCKER_CONTEXT")
            )
        });
        Ok(IsolatedCommand {
            program,
            args,
            cwd: Some(self.home.clone()),
            extra_env,
            policy: EnvPolicy::QualifiedAcquisition,
        })
    }
}
