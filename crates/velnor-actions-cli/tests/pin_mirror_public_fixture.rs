// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;
use tempfile::TempDir;
use velnor_actions_mise::{COSIGN_VERSION, MISE_VERSION, PinnedTool, ToolCatalog};

pub(crate) const POLICY: &str = ".velnor/version-policy.toml";
pub(crate) const INVENTORY: &str = ".velnor/freshness-inventory.json";
pub(crate) const CATALOG: &str = "crates/velnor-actions-mise/src/catalog.rs";
pub(crate) const RUNNER_SOURCE: &str = "crates/velnor-actions-contract/src/config/workflow.rs";
const CONST_SOURCES: [&str; 8] = [
    CATALOG,
    "crates/velnor-actions-contract/src/config/jactionlint_verifier.rs",
    "crates/velnor-actions-actionlint/src/actions.rs",
    "crates/velnor-actions-actionlint/src/tools.rs",
    "crates/velnor-actions-actionlint/src/capabilities.rs",
    RUNNER_SOURCE,
    "crates/velnor-actions-workflow-renderer/src/render.rs",
    "crates/velnor-actions-workflow-renderer/src/steps.rs",
];

pub(crate) type TestResult = Result<(), Box<dyn Error>>;

pub(crate) struct Fixture {
    dir: TempDir,
}

pub(crate) struct Outcome {
    pub(crate) success: bool,
    pub(crate) rows: Vec<Value>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Outcome {
    pub(crate) fn rows_of(&self, check: &str, status: &str) -> Vec<&Value> {
        self.rows
            .iter()
            .filter(|row| row["check"] == check && row["status"] == status)
            .collect()
    }

    pub(crate) fn row(&self, check: &str, status: &str, subject: &str) -> Option<&Value> {
        self.rows_of(check, status).into_iter().find(|row| {
            row["subject"]
                .as_str()
                .is_some_and(|s| s.starts_with(subject))
        })
    }
}

impl Fixture {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = tempfile::tempdir()?;
        for relative in CONST_SOURCES.iter().chain(&[POLICY, INVENTORY]) {
            let target = dir.path().join(relative);
            fs::create_dir_all(target.parent().ok_or("fixture path has no parent")?)?;
            fs::copy(source.join(relative), target)?;
        }
        Ok(Self { dir })
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    pub(crate) fn rewrite(
        &self,
        relative: &str,
        edit: impl FnOnce(String) -> String,
    ) -> TestResult {
        let text = fs::read_to_string(self.path(relative))?;
        let edited = edit(text.clone());
        assert_ne!(text, edited, "edit of {relative} changed nothing");
        fs::write(self.path(relative), edited)?;
        Ok(())
    }

    pub(crate) fn edit_inventory(&self, edit: impl FnOnce(&mut Value)) -> TestResult {
        let mut inventory: Value = serde_json::from_slice(&fs::read(self.path(INVENTORY))?)?;
        edit(&mut inventory);
        fs::write(self.path(INVENTORY), serde_json::to_vec_pretty(&inventory)?)?;
        Ok(())
    }

    pub(crate) fn git(&self, args: &[&str]) -> TestResult {
        let output = clean_command("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(self.dir.path())
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into())
        }
    }

    pub(crate) fn commit_all(&self) -> TestResult {
        self.git(&["init", "-b", "main"])?;
        self.git(&["add", "-A"])?;
        self.git(&["commit", "-m", "base"])
    }

    pub(crate) fn run(&self, base: Option<&str>) -> Result<Outcome, Box<dyn Error>> {
        let mut command = clean_command(env!("CARGO_BIN_EXE_velnor-actions"));
        command
            .env("VELNOR_INTERNAL_OP", "repo-policy-v1")
            .env("VELNOR_REPO_POLICY_ACTION", "pin-mirror")
            .env("VELNOR_REPO_POLICY_ROOT", self.dir.path());
        if let Some(base) = base {
            command.env("VELNOR_REPO_POLICY_BASE_REF", base);
        }
        let output = command.output()?;
        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let rows = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("row: "))
            .map(serde_json::from_str)
            .collect::<Result<Vec<Value>, _>>()?;
        Ok(Outcome {
            success: output.status.success(),
            rows,
            stdout,
            stderr,
        })
    }
}

fn clean_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    command
}

pub(crate) fn compiled_tool_pins() -> Vec<(&'static str, String)> {
    let catalog = ToolCatalog::pinned();
    PinnedTool::ALL
        .into_iter()
        .map(|tool| (tool.tool_name(), catalog.version(tool).to_owned()))
        .chain([
            ("mise", MISE_VERSION.to_owned()),
            ("cosign", COSIGN_VERSION.to_owned()),
        ])
        .collect()
}

pub(crate) fn age_all_evidence(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if key == "checked_at" {
                    *child = Value::String("2000-01-01T00:00:00Z".to_owned());
                } else {
                    age_all_evidence(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(age_all_evidence),
        _ => {}
    }
}

pub(crate) fn set_first_line(text: &str, prefix: &str, replacement: &str) -> String {
    let mut replaced = false;
    text.lines()
        .map(|line| {
            if !replaced && line.starts_with(prefix) {
                replaced = true;
                replacement.to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

pub(crate) fn set_policy_tool(text: &str, name: &str, value: &str) -> String {
    set_first_line(
        text,
        &format!("{name} = "),
        &format!("{name} = \"{value}\""),
    )
}

pub(crate) fn compiled_crane() -> String {
    ToolCatalog::pinned().version(PinnedTool::Crane).to_owned()
}
