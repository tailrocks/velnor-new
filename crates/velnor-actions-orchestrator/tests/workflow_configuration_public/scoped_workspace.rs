// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_orchestrator::{
    MergeContext, PlanOutputMode, merge_internal_with_context, merge_passed, plan_internal,
    plan_outputs,
};

use super::workflow_configuration_fixture::{git, git_fixture, make_repo};

pub(super) type TestResult = Result<(), Box<dyn Error>>;

pub(super) const PACKAGES: [&str; 10] = [
    "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "core", "util",
];

pub(super) const FORCED_AND_DEPENDENTS: [&str; 4] = ["a2", "a5", "a7", "util"];

pub(super) const CORE_CLOSURE: [&str; 6] = ["a1", "a3", "a4", "a5", "a6", "core"];

pub(super) const DOCS_CONFIG: &str = "\n[stacks.rust]\nnon_inputs = [\"docs/**\"]\n";

pub(super) struct ScopedRepo {
    pub(super) dir: TempDir,
    pub(super) base: String,
}

impl ScopedRepo {
    pub(super) fn root(&self) -> &Path {
        self.dir.path()
    }
}

pub(super) struct PlanRun {
    pub(super) response: String,
    pub(super) value: Value,
}

impl PlanRun {
    pub(super) fn jobs(&self) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
        let jobs = self.value["plan"]["jobs"]
            .as_array()
            .ok_or("plan response has no job dispositions")?;
        jobs.iter()
            .map(|job| {
                let id = job["job_id"].as_str().ok_or("plan job has no id")?;
                let disposition = job["disposition"]
                    .as_str()
                    .ok_or("plan job has no disposition")?;
                Ok((id.to_owned(), disposition.to_owned()))
            })
            .collect()
    }

    pub(super) fn with_disposition(
        &self,
        disposition: &str,
    ) -> Result<BTreeSet<String>, Box<dyn Error>> {
        Ok(self
            .jobs()?
            .into_iter()
            .filter(|(_, actual)| actual == disposition)
            .filter_map(|(id, _)| id.strip_prefix("rust-").map(str::to_owned))
            .collect())
    }

    pub(super) fn warnings(&self) -> Vec<String> {
        self.value["plan"]["warnings"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|warning| warning.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn reasons(&self) -> BTreeSet<String> {
        self.value["plan"]["obligations"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|ob| ob["reason"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn not_affected_output(&self) -> Result<String, Box<dyn Error>> {
        Ok(plan_outputs(&self.response, PlanOutputMode::Static)?.not_affected_jobs)
    }
}

pub(super) fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

pub(super) fn union(parts: &[&[&str]]) -> BTreeSet<String> {
    parts
        .iter()
        .flat_map(|part| part.iter())
        .map(|item| (*item).to_owned())
        .collect()
}

pub(super) fn put(root: &Path, path: &str, body: &str) -> TestResult {
    let file = root.join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(file, body)?;
    Ok(())
}

pub(super) fn head(root: &Path) -> Result<String, Box<dyn Error>> {
    let output = git_fixture::command(root)?
        .args(["rev-parse", "HEAD"])
        .output()?;
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub(super) fn commit(root: &Path, message: &str) -> Result<String, Box<dyn Error>> {
    git(&["add", "-A"], root)?;
    git(&["commit", "--allow-empty", "-m", message], root)?;
    head(root)
}

pub(super) fn generate_lockfile(root: &Path) -> TestResult {
    let status = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .status()?;
    assert!(status.success(), "generate-lockfile failed");
    Ok(())
}

pub(super) fn package_manifest(path: &str, name: &str, tables: &str) -> (String, String) {
    (
        format!("{path}/Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{tables}"),
    )
}

pub(super) fn scoped_workspace(extra_config: &str) -> Result<ScopedRepo, Box<dyn Error>> {
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{extra_config}"
    );
    let dir = make_repo(&config)?;
    let root = dir.path();
    fs::remove_dir_all(root.join("src"))?;
    let members = PACKAGES
        .iter()
        .map(|name| {
            if name.starts_with('a') {
                format!("\"apps/{name}\"")
            } else {
                format!("\"libs/{name}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    put(
        root,
        "Cargo.toml",
        &format!("[workspace]\nmembers = [{members}]\nresolver = \"2\"\n"),
    )?;
    let core = "core = { path = \"../../libs/core\" }\n";
    let tables = [
        ("libs/core", "core", String::new()),
        ("libs/util", "util", String::new()),
        ("apps/a1", "a1", format!("[dependencies]\n{core}")),
        (
            "apps/a2",
            "a2",
            "[dependencies]\nutil = { path = \"../../libs/util\" }\n".to_owned(),
        ),
        ("apps/a3", "a3", format!("[dev-dependencies]\n{core}")),
        ("apps/a4", "a4", format!("[dependencies]\n{core}")),
        (
            "apps/a5",
            "a5",
            format!("[target.'cfg(unix)'.dependencies]\n{core}"),
        ),
        (
            "apps/a6",
            "a6",
            "[features]\nextra = [\"dep:core\"]\n[dependencies]\ncore = { path = \"../../libs/core\", optional = true }\n"
                .to_owned(),
        ),
        ("apps/a7", "a7", String::new()),
        ("apps/a8", "a8", String::new()),
    ];
    for (path, name, deps) in &tables {
        let (manifest, body) = package_manifest(path, name, deps);
        put(root, &manifest, &body)?;
        put(root, &format!("{path}/src/lib.rs"), "pub fn f() {}\n")?;
    }
    put(root, "libs/util/build.rs", "fn main() {}\n")?;
    put(root, "apps/a1/data.txt", "data\n")?;
    put(
        root,
        "apps/a7/src/lib.rs",
        "pub const DATA: &str = include_str!(\"../../a1/data.txt\");\n",
    )?;
    put(
        root,
        "apps/a5/src/lib.rs",
        "pub fn up() -> Option<&'static std::path::Path> {\n    std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).parent()\n}\n",
    )?;
    put(root, "docs/guide.md", "guide\n")?;
    put(root, "docs/data/schema.json", "{}\n")?;
    put(root, "README.md", "readme\n")?;
    generate_lockfile(root)?;
    let base = commit(root, "base")?;
    Ok(ScopedRepo { dir, base })
}

pub(super) fn plan(
    root: &Path,
    event: &str,
    base: Option<&str>,
    head: &str,
) -> Result<PlanRun, Box<dyn Error>> {
    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r190001-a1",
        "base": base,
        "head": head,
        "event": event,
        "root": root.to_string_lossy(),
    });
    plan_request(&request)
}

pub(super) fn plan_request(request: &Value) -> Result<PlanRun, Box<dyn Error>> {
    let response = plan_internal(&serde_json::to_string(request)?)?;
    let value = serde_json::from_str(&response)?;
    Ok(PlanRun { response, value })
}

pub(super) fn merge_verdict(
    run: &PlanRun,
    event: &str,
    conclusions: &BTreeMap<String, &str>,
) -> Result<Value, Box<dyn Error>> {
    let run_key = run.value["plan"]["run_key"]
        .as_str()
        .ok_or("plan response has no run key")?;
    let ids: Vec<String> = conclusions.keys().cloned().collect();
    let required_jobs: Vec<Value> = conclusions
        .iter()
        .map(|(id, conclusion)| json!({ "job_id": id, "conclusion": conclusion }))
        .collect();
    let request = json!({
        "schema": 1,
        "run_key": run_key,
        "actual_event": event,
        "plan": run.value["plan"],
        "matrix": run.value["matrix"],
        "matrix_reports": [],
        "task_reports": [],
        "required_job_ids": ids,
        "required_jobs": required_jobs,
    });
    let merged =
        merge_internal_with_context(&serde_json::to_string(&request)?, &MergeContext::detached())?;
    Ok(serde_json::from_str(&merged)?)
}

pub(super) fn verdict_passed(report: &Value) -> Result<bool, Box<dyn Error>> {
    Ok(merge_passed(&report.to_string())?)
}

pub(super) fn miss_reasons(report: &Value) -> BTreeSet<String> {
    report["miss_reasons"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|reason| reason.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn crate_conclusions(
    run: &PlanRun,
    reported: &dyn Fn(&str, &str) -> &'static str,
) -> Result<BTreeMap<String, &'static str>, Box<dyn Error>> {
    Ok(run
        .jobs()?
        .into_iter()
        .filter(|(id, _)| id.starts_with("rust-"))
        .map(|(id, disposition)| {
            let conclusion = reported(&id, &disposition);
            (id, conclusion)
        })
        .collect())
}
