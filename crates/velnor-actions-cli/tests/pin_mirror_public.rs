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
use velnor_actions_freshness::BaseRef;
use velnor_actions_mise::{COSIGN_VERSION, MISE_VERSION, PinnedTool, ToolCatalog};

const POLICY: &str = ".velnor/version-policy.toml";
const INVENTORY: &str = ".velnor/freshness-inventory.json";
const CATALOG: &str = "crates/velnor-actions-mise/src/catalog.rs";
const CONST_SOURCES: [&str; 8] = [
    CATALOG,
    "crates/velnor-actions-contract/src/config/jactionlint_verifier.rs",
    "crates/velnor-actions-actionlint/src/actions.rs",
    "crates/velnor-actions-actionlint/src/tools.rs",
    "crates/velnor-actions-actionlint/src/capabilities.rs",
    "crates/velnor-actions-contract/src/config/workflow.rs",
    "crates/velnor-actions-workflow-renderer/src/render.rs",
    "crates/velnor-actions-workflow-renderer/src/steps.rs",
];

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture {
    dir: TempDir,
}

struct Outcome {
    success: bool,
    rows: Vec<Value>,
}

impl Outcome {
    fn rows_of(&self, check: &str, status: &str) -> Vec<&Value> {
        self.rows
            .iter()
            .filter(|row| row["check"] == check && row["status"] == status)
            .collect()
    }

    fn row(&self, check: &str, status: &str, subject: &str) -> Option<&Value> {
        self.rows_of(check, status).into_iter().find(|row| {
            row["subject"]
                .as_str()
                .is_some_and(|s| s.starts_with(subject))
        })
    }
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = tempfile::tempdir()?;
        for relative in CONST_SOURCES.iter().chain(&[POLICY, INVENTORY]) {
            let target = dir.path().join(relative);
            fs::create_dir_all(target.parent().ok_or("fixture path has no parent")?)?;
            fs::copy(source.join(relative), target)?;
        }
        Ok(Self { dir })
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn rewrite(&self, relative: &str, edit: impl FnOnce(String) -> String) -> TestResult {
        let text = fs::read_to_string(self.path(relative))?;
        let edited = edit(text.clone());
        assert_ne!(text, edited, "edit of {relative} changed nothing");
        fs::write(self.path(relative), edited)?;
        Ok(())
    }

    fn edit_inventory(&self, edit: impl FnOnce(&mut Value)) -> TestResult {
        let mut inventory: Value = serde_json::from_slice(&fs::read(self.path(INVENTORY))?)?;
        edit(&mut inventory);
        fs::write(self.path(INVENTORY), serde_json::to_vec_pretty(&inventory)?)?;
        Ok(())
    }

    fn git(&self, args: &[&str]) -> TestResult {
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

    fn commit_all(&self) -> TestResult {
        self.git(&["init", "-b", "main"])?;
        self.git(&["add", "-A"])?;
        self.git(&["commit", "-m", "base"])
    }

    fn run(&self, base: Option<&str>) -> Result<Outcome, Box<dyn Error>> {
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
        let rows = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("row: "))
            .map(serde_json::from_str)
            .collect::<Result<Vec<Value>, _>>()?;
        Ok(Outcome {
            success: output.status.success(),
            rows,
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

fn compiled_tool_pins() -> Vec<(&'static str, String)> {
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

fn age_all_evidence(value: &mut Value) {
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

fn set_policy_tool(text: &str, name: &str, value: &str) -> String {
    let prefix = format!("{name} = ");
    let mut replaced = false;
    text.lines()
        .map(|line| {
            if !replaced && line.starts_with(&prefix) {
                replaced = true;
                format!("{prefix}\"{value}\"")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn pin_mirror_passes_shipped_pins_ignoring_evidence_age() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.edit_inventory(age_all_evidence)?;
    let outcome = fixture.run(None)?;
    assert!(outcome.success, "shipped pins rejected: {:?}", outcome.rows);
    for (name, version) in compiled_tool_pins() {
        let row = outcome
            .row("policy-mirror", "pass", &format!("tool {name}"))
            .ok_or_else(|| io::Error::other(format!("policy-mirror pass row missing: {name}")))?;
        assert_eq!(row["detail"], version.as_str(), "policy pin for {name}");
    }
    assert_eq!(outcome.rows_of("pin-diff", "info").len(), 0);
    Ok(())
}

#[test]
fn pin_mirror_rejects_drift_for_every_tool_the_old_gate_covered() -> TestResult {
    for (name, _) in compiled_tool_pins() {
        let fixture = Fixture::new()?;
        fixture.rewrite(POLICY, |text| set_policy_tool(&text, name, "999.0.0"))?;
        let outcome = fixture.run(None)?;
        assert!(!outcome.success, "policy drift accepted for {name}");
        assert!(
            outcome
                .row("policy-mirror", "fail", &format!("tool {name}"))
                .is_some(),
            "policy drift for {name} not named: {:?}",
            outcome.rows
        );

        let fixture = Fixture::new()?;
        fixture.edit_inventory(|inventory| {
            for tool in inventory["tools"].as_array_mut().into_iter().flatten() {
                if tool["name"] == name {
                    tool["pinned"] = Value::String("999.0.0".to_owned());
                }
            }
        })?;
        let outcome = fixture.run(None)?;
        assert!(!outcome.success, "inventory drift accepted for {name}");
        assert!(
            outcome
                .row("local-pin", "fail", &format!("tool {name}"))
                .is_some(),
            "inventory drift for {name} not named: {:?}",
            outcome.rows
        );
    }
    Ok(())
}

#[test]
fn pin_mirror_rejects_action_and_runner_drift() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.rewrite(POLICY, |text| {
        text.replacen(
            "sha = \"6be3c7ed4c21d4f76b208d351d95aac273ebb76d\"",
            "sha = \"0000000000000000000000000000000000000000\"",
            1,
        )
    })?;
    let outcome = fixture.run(None)?;
    assert!(!outcome.success);
    assert!(outcome.row("policy-mirror", "fail", "action ").is_some());

    let fixture = Fixture::new()?;
    fixture.rewrite(POLICY, |text| {
        text.replacen(
            "default = \"ubuntu-26.04\"",
            "default = \"ubuntu-24.04\"",
            1,
        )
    })?;
    let outcome = fixture.run(None)?;
    assert!(!outcome.success);
    assert!(
        outcome
            .row("policy-mirror", "fail", "runner default")
            .is_some()
    );
    Ok(())
}

#[test]
fn pin_mirror_reports_pin_diff_without_changing_verdict() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.commit_all()?;

    let unchanged = fixture.run(Some("HEAD"))?;
    assert!(unchanged.success);
    let rows = unchanged.rows_of("pin-diff", "info");
    assert_eq!(rows.len(), 1, "unexpected rows: {rows:?}");
    assert!(
        rows[0]["detail"]
            .as_str()
            .is_some_and(|d| d.starts_with("unchanged vs"))
    );

    fixture.rewrite(POLICY, |text| set_policy_tool(&text, "crane", "0.22.2"))?;
    fixture.rewrite(CATALOG, |text| {
        text.replacen(
            "CRANE_VERSION: &str = \"0.22.1\"",
            "CRANE_VERSION: &str = \"0.22.2\"",
            1,
        )
    })?;
    fixture.edit_inventory(|inventory| {
        for tool in inventory["tools"].as_array_mut().into_iter().flatten() {
            if tool["name"] == "crane" {
                tool["pinned"] = Value::String("0.22.2".to_owned());
            }
        }
    })?;
    let bumped = fixture.run(Some("refs/heads/main"))?;
    assert!(
        bumped.success,
        "consistent bump rejected: {:?}",
        bumped.rows
    );
    let diff = bumped.rows_of("pin-diff", "info");
    assert_eq!(diff.len(), 2, "unexpected rows: {diff:?}");
    for subject in ["policy tool crane", "inventory tool crane"] {
        let row = bumped
            .row("pin-diff", "info", subject)
            .ok_or_else(|| io::Error::other(format!("pin-diff row missing: {subject}")))?;
        assert_eq!(row["detail"], "base=0.22.1 head=0.22.2");
    }

    fixture.rewrite(POLICY, |text| set_policy_tool(&text, "crane", "0.22.3"))?;
    let drifted = fixture.run(Some("HEAD"))?;
    assert!(!drifted.success, "drift accepted despite a valid base");
    Ok(())
}

#[test]
fn pin_mirror_marks_missing_base_files_absent_and_fails_closed_on_git_problems() -> TestResult {
    let fixture = Fixture::new()?;
    fs::rename(fixture.path(".velnor"), fixture.path("held-velnor"))?;
    fixture.commit_all()?;
    fs::rename(fixture.path("held-velnor"), fixture.path(".velnor"))?;
    let outcome = fixture.run(Some("HEAD"))?;
    assert!(
        outcome.success,
        "absent base files rejected: {:?}",
        outcome.rows
    );
    let row = outcome
        .row("pin-diff", "info", "policy tool crane")
        .ok_or("absent-base row missing")?;
    assert_eq!(row["detail"], "base=<absent> head=0.22.1");

    let unresolved = fixture.run(Some("refs/heads/missing"))?;
    assert!(!unresolved.success);
    assert!(
        unresolved
            .row("pin-diff", "fail", "refs/heads/missing")
            .is_some()
    );

    let fixture = Fixture::new()?;
    let real = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(POLICY);
    fs::remove_file(fixture.path(POLICY))?;
    std::os::unix::fs::symlink("schema=1", fixture.path(POLICY))?;
    fixture.commit_all()?;
    fs::remove_file(fixture.path(POLICY))?;
    fs::copy(&real, fixture.path(POLICY))?;
    let linked = fixture.run(Some("HEAD"))?;
    assert!(!linked.success, "symlinked base policy accepted");
    assert!(linked.row("pin-diff", "fail", "HEAD").is_some());
    Ok(())
}

#[test]
fn base_ref_parse_rejects_unsafe_refs() -> TestResult {
    for accepted in ["origin/main", "HEAD", "refs/heads/a-b"] {
        BaseRef::parse(accepted).map_err(io::Error::other)?;
    }
    let long = "a".repeat(201);
    for rejected in [
        "",
        "-x",
        "a..b",
        "HEAD@{0}",
        "a b",
        "r\u{e9}f",
        long.as_str(),
    ] {
        assert!(BaseRef::parse(rejected).is_err(), "accepted {rejected:?}");
    }

    let fixture = Fixture::new()?;
    assert!(!fixture.run(Some("--output=x"))?.success);
    let empty = fixture.run(Some(""))?;
    assert!(empty.success);
    assert_eq!(empty.rows_of("pin-diff", "info").len(), 0);
    Ok(())
}
