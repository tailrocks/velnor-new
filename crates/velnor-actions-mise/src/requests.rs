use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{
    IsolatedCommand, ProcessOutput, mise_install_argv_tail, redact_argv_for_debug,
};
use crate::error::MiseError;
use std::ffi::{OsStr, OsString};
#[path = "requests_helpers.rs"]
mod helpers;
use helpers::full_mise_argv;
#[path = "requests_metadata.rs"]
mod metadata;
pub use metadata::{MetadataDiscovery, MetadataQualification};

#[derive(Clone, PartialEq, Eq)]
pub struct PinnedToolExec {
    tools: Vec<PinnedTool>,
    program: OsString,
    args: Vec<OsString>,
}

impl std::fmt::Debug for PinnedToolExec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedToolExec")
            .field("tools", &self.tools)
            .field("program", &self.program)
            .field("args", &redact_argv_for_debug(&self.args))
            .finish()
    }
}

const FORBIDDEN_PROGRAM_STEM: &str = "rustup";

impl PinnedToolExec {
    pub fn new(
        tools: Vec<PinnedTool>,
        program: &OsStr,
        args: Vec<OsString>,
    ) -> Result<Self, MiseError> {
        if tools.is_empty() {
            return Err(MiseError::EmptyToolchain);
        }
        if program.is_empty() {
            return Err(MiseError::EmptyCommand {
                program: "mise".to_owned(),
            });
        }
        reject_forbidden_payload(program, &args)?;
        Ok(Self {
            tools,
            program: program.to_owned(),
            args,
        })
    }

    #[must_use]
    pub fn tools(&self) -> &[PinnedTool] {
        &self.tools
    }

    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    #[must_use]
    pub fn payload(&self) -> Vec<OsString> {
        let mut payload = Vec::with_capacity(self.args.len() + 1);
        payload.push(self.program.clone());
        payload.extend(self.args.iter().cloned());
        payload
    }

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(catalog, &self.tools, &self.payload())
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&self.tools);
        let command = IsolatedCommand::mise_exec(&specs, &self.payload())?;
        if self.tools.as_slice() == [PinnedTool::Gh] {
            return Ok(command.with_policy(crate::command::EnvPolicy::GhApi));
        }
        Ok(command)
    }

    pub fn command_with_github_token(
        &self,
        catalog: &ToolCatalog,
        token: &str,
    ) -> Result<IsolatedCommand, MiseError> {
        if self.tools.as_slice() != [PinnedTool::Gh] || self.program != "gh" {
            return Err(MiseError::InvalidStepInput {
                field: "GH_TOKEN".to_owned(),
                value: "credential_requires_pinned_gh".to_owned(),
            });
        }
        self.command(catalog)?.with_github_token(token)
    }

    pub fn run(&self, catalog: &ToolCatalog) -> Result<ProcessOutput, MiseError> {
        self.command(catalog)?.run()
    }
}

fn reject_forbidden_payload(program: &OsStr, args: &[OsString]) -> Result<(), MiseError> {
    let stem = std::path::Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    if stem == FORBIDDEN_PROGRAM_STEM {
        return Err(MiseError::ForbiddenPayload {
            program: program.to_string_lossy().into_owned(),
            reason: "rustup_forbidden".to_owned(),
        });
    }
    let is_cargo_install =
        program == OsStr::new("cargo") && args.first().is_some_and(|arg| arg == "install");
    if is_cargo_install {
        return Err(MiseError::ForbiddenPayload {
            program: "cargo".to_owned(),
            reason: "cargo_install_forbidden".to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseInstall {
    tools: Vec<PinnedTool>,
}

impl MiseInstall {
    pub fn new(tools: Vec<PinnedTool>) -> Result<Self, MiseError> {
        if tools.is_empty() {
            return Err(MiseError::EmptyToolchain);
        }
        Ok(Self { tools })
    }

    #[must_use]
    pub fn tools(&self) -> &[PinnedTool] {
        &self.tools
    }

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        let specs = catalog.tool_specs(&self.tools);
        let mut argv = vec![OsString::from("mise")];
        argv.extend(mise_install_argv_tail(&specs));
        argv
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&self.tools);
        IsolatedCommand::mise_install(&specs)
    }

    pub fn run(&self, catalog: &ToolCatalog) -> Result<ProcessOutput, MiseError> {
        self.command(catalog)?.run()
    }
}
