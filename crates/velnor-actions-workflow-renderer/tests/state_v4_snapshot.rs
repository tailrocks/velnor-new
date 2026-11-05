// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;
use std::process::Command;

use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::cache_family::S256;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::plan_sources::cache_keys::{StateRootPlan, cache_keys_step};

use crate::gc1_scene::fail;

const ROOTS: [&str; 2] = ["ws-a", "ws-b"];

fn write(repo: &Path, path: &str, text: &str) -> Result<(), Box<dyn Error>> {
    let target = repo.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target, text)?;
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(fail(&format!("git {args:?} failed")))
    }
}

fn commit(repo: &Path) -> Result<(), Box<dyn Error>> {
    git(repo, &["add", "-A"])?;
    git(repo, &["commit", "-q", "--allow-empty", "-m", "fixture"])
}

fn fixture(repo: &Path) -> Result<(), Box<dyn Error>> {
    git(repo, &["init", "-q", "-b", "main"])?;
    for root in ROOTS {
        write(
            repo,
            &format!("{root}/Cargo.toml"),
            "[workspace]\nmembers = [\"crates/m\"]\n",
        )?;
        write(repo, &format!("{root}/Cargo.lock"), "version = 4\n")?;
        write(
            repo,
            &format!("{root}/crates/m/Cargo.toml"),
            "[package]\nname = \"m\"\n",
        )?;
        write(
            repo,
            &format!("{root}/crates/m/src/lib.rs"),
            "pub fn f() {}\n",
        )?;
        write(repo, &format!("{root}/.cargo/config.toml"), "[build]\n")?;
        write(
            repo,
            &format!("{root}/rust-toolchain.toml"),
            "[toolchain]\n",
        )?;
        write(repo, &format!("{root}/declared/input.txt"), "declared\n")?;
    }
    write(repo, "docs/guide.md", "guide\n")?;
    write(repo, "README.md", "readme\n")?;
    write(repo, ".github/workflows/ci.yml", "name: CI\n")?;
    commit(repo)
}

fn plans(task_set: &str, recipe: &str) -> Vec<StateRootPlan> {
    ROOTS
        .iter()
        .map(|root| StateRootPlan {
            root: (*root).to_owned(),
            pathspecs: vec![
                format!("{root}/Cargo.toml"),
                format!("{root}/.cargo"),
                format!(":(glob){root}/rust-toolchain*"),
                format!("{root}/crates/m"),
                format!(":(glob){root}/declared/**"),
            ],
            task_set: S256::of(format!("{root}{task_set}").as_bytes()),
            producer_recipe: S256::of(format!("{root}{recipe}").as_bytes()),
        })
        .collect()
}

fn snapshots(
    repo: &Path,
    scratch: &Path,
    task_set: &str,
    recipe: &str,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let roots: Vec<String> = ROOTS.iter().map(|root| (*root).to_owned()).collect();
    let step = cache_keys_step(
        &ToolCatalog::pinned(),
        &roots,
        &[],
        &plans(task_set, recipe),
    )
    .map_err(|error| fail(&format!("{error:?}")))?
    .ok_or_else(|| fail("cache keys step vanished"))?;
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(fail("cache keys step left its shell form"));
    };
    let output = scratch.join("output");
    drop(std::fs::remove_file(&output));
    let temp = scratch.join("temp");
    std::fs::create_dir_all(&temp)?;
    let status = Command::new(&run[0])
        .args(&run[1..])
        .current_dir(repo)
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_WORKSPACE", repo)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("ImageOS", "ubuntu24")
        .env("VELNOR_MBX_BUNDLE_VERSION", "1.2.3")
        .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.89.0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()?;
    if !status.success() {
        return Err(fail("the cache keys step failed"));
    }
    let text = std::fs::read_to_string(&output)?;
    let found: BTreeMap<String, String> = text
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(name, _)| name.starts_with("state_") && name.ends_with("_snapshot"))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    if found.len() != ROOTS.len() || found.values().any(|value| S256::parse(value).is_err()) {
        return Err(fail(&format!("snapshots malformed: {found:?}")));
    }
    Ok(found)
}

type Edit = fn(&Path) -> Result<(), Box<dyn Error>>;

fn append(repo: &Path, path: &str) -> Result<(), Box<dyn Error>> {
    let target = repo.join(path);
    let mut text = std::fs::read_to_string(&target)?;
    text.push_str("// edit\n");
    std::fs::write(target, text)?;
    Ok(())
}

type Row = (&'static str, Edit, &'static [&'static str]);

const ONLY_A: &[&str] = &["state_slice_ws_a_snapshot"];
const ONLY_B: &[&str] = &["state_slice_ws_b_snapshot"];
const NONE: &[&str] = &[];

fn rows() -> [Row; 12] {
    [
        (
            "member source byte",
            |r| append(r, "ws-a/crates/m/src/lib.rs"),
            ONLY_A,
        ),
        ("Cargo.lock", |r| append(r, "ws-a/Cargo.lock"), ONLY_A),
        (
            "cargo config",
            |r| append(r, "ws-a/.cargo/config.toml"),
            ONLY_A,
        ),
        (
            "rust-toolchain",
            |r| append(r, "ws-b/rust-toolchain.toml"),
            ONLY_B,
        ),
        (
            "workspace manifest",
            |r| append(r, "ws-b/Cargo.toml"),
            ONLY_B,
        ),
        (
            "declared input glob",
            |r| append(r, "ws-a/declared/input.txt"),
            ONLY_A,
        ),
        (
            "renamed source",
            |r| {
                Ok(std::fs::rename(
                    r.join("ws-b/crates/m/src/lib.rs"),
                    r.join("ws-b/crates/m/src/main.rs"),
                )?)
            },
            ONLY_B,
        ),
        ("docs", |r| append(r, "docs/guide.md"), NONE),
        ("readme", |r| append(r, "README.md"), NONE),
        (
            "workflow file",
            |r| append(r, ".github/workflows/ci.yml"),
            NONE,
        ),
        (
            "untracked build output",
            |r| write(r, "ws-a/target/debug/x", "built\n"),
            NONE,
        ),
        (
            "touched mtime only",
            |r| {
                let file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(r.join("ws-a/crates/m/src/lib.rs"))?;
                file.set_modified(std::time::SystemTime::now())?;
                Ok(())
            },
            NONE,
        ),
    ]
}

#[test]
#[cfg(unix)]
fn snapshot_tracks_only_rust_inputs() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let repo = dir.path().join("repo");
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&repo)?;
    std::fs::create_dir_all(&scratch)?;
    fixture(&repo)?;
    let base = snapshots(&repo, &scratch, "", "")?;
    if base != snapshots(&repo, &scratch, "", "")? {
        return Err(fail("snapshot unstable across runs"));
    }
    for (label, edit, changed) in rows() {
        git(&repo, &["checkout", "-q", "--", "."])?;
        git(&repo, &["clean", "-fdq", "-e", "ws-a/target"])?;
        edit(&repo)?;
        commit(&repo)?;
        let now = snapshots(&repo, &scratch, "", "")?;
        for (name, before) in &base {
            let moved = now.get(name) != Some(before);
            let expected = changed.contains(&name.as_str());
            if moved != expected {
                return Err(fail(&format!(
                    "{label}: {name} changed={moved}, expected changed={expected}"
                )));
            }
        }
        git(&repo, &["reset", "-q", "--hard", "HEAD~1"])?;
    }
    let argv = snapshots(&repo, &scratch, "-argv-changed", "")?;
    if argv.keys().any(|name| argv.get(name) == base.get(name)) {
        return Err(fail("a changed producer task set left a snapshot equal"));
    }
    let recipe = snapshots(&repo, &scratch, "", "-recipe-changed")?;
    if recipe.keys().any(|name| recipe.get(name) == base.get(name)) {
        return Err(fail("a changed producer recipe left a snapshot equal"));
    }
    Ok(())
}
