// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::JACTIONLINT_VERSION;

pub const SHELLCHECK_VERSION: &str = "0.11.0";

pub const JACTIONLINT_CONFIG_PATH: &str = ".github/jactionlint.yaml";

pub const LINT_FILE_GLOBS: [&str; 3] = [
    ".github/workflows/*.y*ml",
    ".github/actions/**/action.y*ml",
    ".github/dependabot.y*ml",
];

pub const LINT_FLAGS: [&str; 5] = [
    "--strict-exit",
    "--no-online",
    "--shellcheck=shellcheck",
    "--pyflakes=",
    "--no-color",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JactionlintToolchain {
    version: String,
}

impl JactionlintToolchain {
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            version: JACTIONLINT_VERSION.to_owned(),
        }
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn mise_tool_spec(&self) -> String {
        format!("jactionlint@{}", self.version)
    }

    #[must_use]
    pub fn staged_lint_argv(config_path: &str, files: &[String]) -> Vec<String> {
        let mut argv = vec![
            "--config-file".to_owned(),
            config_path.to_owned(),
            "--format".to_owned(),
            "oneline".to_owned(),
        ];
        argv.extend(LINT_FLAGS.iter().map(|flag| (*flag).to_owned()));
        argv.extend(files.iter().cloned());
        argv
    }
}

impl Default for JactionlintToolchain {
    fn default() -> Self {
        Self::pinned()
    }
}

const LINT_FILE_FINDS: [&str; 3] = [
    ".github/workflows -maxdepth 1 -type f -name \"*.y*ml\"",
    ".github/actions -type f -name \"action.y*ml\"",
    ".github -maxdepth 1 -type f -name \"dependabot.y*ml\"",
];

#[must_use]
pub fn workflow_lint_script(jactionlint_command: &str) -> String {
    let finds = LINT_FILE_FINDS
        .iter()
        .map(|args| ["find ", args, "; "].concat())
        .collect::<String>();
    format!(
        "files=(); while IFS= read -r file; do files+=(\"$file\"); done < <({{ {finds}}} 2>/dev/null | LC_ALL=C sort); if [ \"${{#files[@]}}\" -eq 0 ]; then echo \"jactionlint: no workflow files matched\" >&2; exit 3; fi; exec {jactionlint_command} --config-file {JACTIONLINT_CONFIG_PATH} --format github {flags} \"${{files[@]}}\"",
        flags = LINT_FLAGS.join(" "),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellcheckToolchain {
    version: String,
}

impl ShellcheckToolchain {
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            version: SHELLCHECK_VERSION.to_owned(),
        }
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn mise_tool_spec(&self) -> String {
        format!("shellcheck@{}", self.version)
    }
}

impl Default for ShellcheckToolchain {
    fn default() -> Self {
        Self::pinned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowLintTools {
    pub jactionlint: JactionlintToolchain,

    pub shellcheck: ShellcheckToolchain,
}

impl WorkflowLintTools {
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            jactionlint: JactionlintToolchain::pinned(),
            shellcheck: ShellcheckToolchain::pinned(),
        }
    }

    #[must_use]
    pub fn mise_tool_specs(&self) -> Vec<String> {
        vec![
            self.jactionlint.mise_tool_spec(),
            self.shellcheck.mise_tool_spec(),
        ]
    }
}
