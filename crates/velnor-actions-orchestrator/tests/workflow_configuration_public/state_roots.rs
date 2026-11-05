// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_ROOT_ENV, MBX_BUNDLE_SNAPSHOT_ENV,
};
use velnor_actions_contract::{Job, StepId, StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::one_job_per_crate::mbx_repo;
use super::workflow_configuration_fixture::{add_independent_mbx_package, git, write_nextest_task};

fn render(extra_roots: &[&str]) -> Result<BTreeMap<String, Job>, Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    for (index, dir) in extra_roots.iter().enumerate() {
        add_independent_mbx_package(repo.path(), dir, &format!("extra{index}"))?;
    }
    let prepared = prepare(repo.path())?;
    Ok(finalized_jobs(&prepared)?)
}

fn snapshot_bindings(jobs: &BTreeMap<String, Job>) -> BTreeSet<(String, String)> {
    jobs.values()
        .flat_map(|job| &job.steps)
        .filter(|step| step.role == Some(StepRole::MbxBundleKey))
        .filter_map(|step| match &step.kind {
            StepKind::Shell { env, .. } => Some((
                env.get(MBX_BUNDLE_ROOT_ENV)?.clone(),
                env.get(MBX_BUNDLE_SNAPSHOT_ENV)?.clone(),
            )),
            _ => None,
        })
        .collect()
}

pub(super) fn plan_key_script(jobs: &BTreeMap<String, Job>) -> Result<String, Box<dyn Error>> {
    let step = jobs
        .get("plan")
        .and_then(|plan| plan.steps.iter().find(|s| s.id == Some(StepId::CacheKeys)))
        .ok_or("plan job lost its cache keys step")?;
    match &step.kind {
        StepKind::Shell { run, .. } => Ok(run.join(" ")),
        _ => Err("cache keys step left its shell form".into()),
    }
}

fn error_of(extra_roots: &[&str]) -> Result<String, Box<dyn Error>> {
    match render(extra_roots) {
        Ok(_) => Err(format!("roots {extra_roots:?} generated a workflow").into()),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn generation_rejects_colliding_or_delimiter_roots() -> Result<(), Box<dyn Error>> {
    let widest = "d".repeat(48);
    let jobs = render(&[widest.as_str()])?;
    let bindings = snapshot_bindings(&jobs);
    let script = plan_key_script(&jobs)?;
    let wanted = [
        (
            "crates/nested",
            "${{needs.plan.outputs.state_slice_crates_nested_snapshot}}",
        ),
        (
            widest.as_str(),
            &format!("${{{{needs.plan.outputs.state_slice_{widest}_snapshot}}}}"),
        ),
    ];
    for (root, expr) in wanted {
        if !bindings.contains(&(root.to_owned(), expr.to_owned())) {
            return Err(format!("no snapshot binding for root {root}: {bindings:?}").into());
        }
        let name = expr
            .trim_start_matches("${{needs.plan.outputs.")
            .trim_end_matches("}}");
        if !script.contains(&format!("{name}=$snap")) {
            return Err(format!("plan never publishes {name}").into());
        }
    }
    for member in [
        "'crates/nested'",
        "'crates/nested/.cargo'",
        "'crates/nested/Cargo.toml'",
    ] {
        if !script.contains(member) {
            return Err(format!("plan snapshot lost its input {member}").into());
        }
    }
    if CacheFamily::MbxBundle.max_key_bytes()
        > velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
    {
        return Err("widest state key outgrew the cache cap".into());
    }
    for (roots, needle) in [
        (vec!["a/b", "a-b"], "root_slug_collision"),
        (vec!["crates/s256-x"], "root_collides_with_digest_delimiter"),
        (vec!["e".repeat(49).leak() as &str], "root_not_sluggable"),
    ] {
        let message = error_of(&roots)?;
        if !message.contains(needle) {
            return Err(format!("roots {roots:?} failed without {needle}: {message}").into());
        }
    }
    Ok(())
}

fn snapshot_names(jobs: &BTreeMap<String, Job>) -> BTreeMap<String, String> {
    snapshot_bindings(jobs)
        .into_iter()
        .map(|(root, expr)| {
            let name = expr
                .trim_start_matches("${{needs.plan.outputs.")
                .trim_end_matches("}}")
                .to_owned();
            (root, name)
        })
        .collect()
}

fn state_snapshots(
    jobs: &BTreeMap<String, Job>,
    repo: &Path,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    git(&["add", "-A"], repo)?;
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("github_output");
    fs::write(&output, "")?;
    let run = Command::new("sh")
        .args(["-c", &plan_key_script(jobs)?])
        .env("RUNNER_TEMP", temp.path())
        .env("GITHUB_WORKSPACE", repo)
        .env("GITHUB_OUTPUT", &output)
        .env("ImageOS", "ubuntu24")
        .env("VELNOR_MBX_BUNDLE_VERSION", "1.2.3")
        .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.89.0")
        .output()?;
    if !run.status.success() {
        return Err(format!(
            "plan script failed: {}",
            String::from_utf8_lossy(&run.stderr)
        )
        .into());
    }
    let snapshots: BTreeMap<String, String> = fs::read_to_string(&output)?
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(name, _)| name.starts_with("state_") && name.ends_with("_snapshot"))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    if snapshots.len() != snapshot_names(jobs).len() || snapshots.len() < 2 {
        return Err(format!("expected a snapshot per root: {snapshots:?}").into());
    }
    Ok(snapshots)
}

fn moved_roots(
    names: &BTreeMap<String, String>,
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<String> {
    names
        .iter()
        .filter(|(_, name)| before.get(*name) != after.get(*name))
        .map(|(root, _)| root.clone())
        .collect()
}

pub(super) fn declared_input_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    add_independent_mbx_package(repo.path(), "other", "other")?;
    write_nextest_task(&repo.path().join("other"))?;
    let config = repo.path().join(".velnor/config.toml");
    let mut text = fs::read_to_string(&config)?;
    text.push_str("\n[[stacks.rust.inputs]]\nglobs = [\"data/**\"]\npackages = [\"nested\"]\n");
    fs::write(&config, text)?;
    fs::create_dir_all(repo.path().join("data"))?;
    fs::write(repo.path().join("data/fixture.txt"), "one\n")?;
    Ok(repo)
}

fn edit_and_measure(
    jobs: &BTreeMap<String, Job>,
    repo: &Path,
    file: &str,
    content: &str,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let path = repo.join(file);
    let original = fs::read_to_string(&path).ok();
    fs::create_dir_all(path.parent().ok_or("file without a parent")?)?;
    fs::write(&path, content)?;
    let measured = state_snapshots(jobs, repo);
    match original {
        Some(text) => fs::write(&path, text)?,
        None => fs::remove_file(&path)?,
    }
    measured
}

#[test]
fn state_snapshot_moves_only_with_the_roots_rust_inputs() -> Result<(), Box<dyn Error>> {
    let repo = declared_input_repo()?;
    let root = repo.path();
    let jobs = finalized_jobs(&prepare(root)?)?;
    let names = snapshot_names(&jobs);
    let base = state_snapshots(&jobs, root)?;
    let cases: [(&str, &str, &[&str]); 5] = [
        ("data/fixture.txt", "two\n", &["crates/nested"]),
        (
            "crates/rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.90.0\"\n",
            &["crates/nested"],
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.90.0\"\n",
            &["crates/nested", "other"],
        ),
        (
            "crates/nested/src/lib.rs",
            "pub fn f() { let _ = 1; }\n",
            &["crates/nested"],
        ),
        ("docs/guide.md", "words\n", &[]),
    ];
    for (file, content, expected) in cases {
        let after = edit_and_measure(&jobs, root, file, content)?;
        let got = moved_roots(&names, &base, &after);
        if got != expected {
            return Err(format!("editing {file} moved {got:?}, wanted {expected:?}").into());
        }
    }
    if state_snapshots(&jobs, root)? != base {
        return Err("restoring every edit did not restore the snapshots".into());
    }
    Ok(())
}

#[test]
fn state_snapshot_moves_with_the_lane_task_digest() -> Result<(), Box<dyn Error>> {
    let repo = declared_input_repo()?;
    let root = repo.path();
    let jobs = finalized_jobs(&prepare(root)?)?;
    let names = snapshot_names(&jobs);
    let base = state_snapshots(&jobs, root)?;
    let config = root.join(".velnor/config.toml");
    let mut text = fs::read_to_string(&config)?;
    text.push_str("\n[stacks.rust]\nclippy_all_features = true\n");
    fs::write(&config, text)?;
    let changed = finalized_jobs(&prepare(root)?)?;
    let after = state_snapshots(&changed, root)?;
    let got = moved_roots(&names, &base, &after);
    if got != ["crates/nested", "other"] {
        return Err(format!("a changed task digest moved {got:?}").into());
    }
    Ok(())
}

#[test]
fn a_cargo_build_jobs_change_invalidates_only_its_roots_state_on_purpose()
-> Result<(), Box<dyn Error>> {
    let repo = declared_input_repo()?;
    let root = repo.path();
    let jobs = finalized_jobs(&prepare(root)?)?;
    let names = snapshot_names(&jobs);
    let base = state_snapshots(&jobs, root)?;
    let config = root.join(".velnor/config.toml");
    let original = fs::read_to_string(&config)?;
    let cases = [
        (
            "build_jobs",
            format!("{original}\n[stacks.rust.crates.nested]\nbuild_jobs = 1\n"),
            vec!["crates/nested"],
        ),
        (
            "state_producer_timeout_minutes",
            original.replacen(
                "plan_timeout_minutes = 15\n",
                "plan_timeout_minutes = 15\nstate_producer_timeout_minutes = 17\n",
                1,
            ),
            vec![],
        ),
    ];
    for (label, edited, expected) in cases {
        if edited == original {
            return Err(format!("{label}: the config edit did not apply").into());
        }
        fs::write(&config, &edited)?;
        let changed = finalized_jobs(&prepare(root)?)?;
        let got = moved_roots(&names, &base, &state_snapshots(&changed, root)?);
        if got != expected {
            return Err(format!("{label} moved {got:?}, wanted {expected:?}").into());
        }
    }
    fs::write(&config, &original)?;
    let restored = finalized_jobs(&prepare(root)?)?;
    if state_snapshots(&restored, root)? != base {
        return Err("restoring the config did not restore the snapshots".into());
    }
    Ok(())
}

#[test]
fn state_snapshot_follows_targets_outside_the_package_directory() -> Result<(), Box<dyn Error>> {
    let repo = declared_input_repo()?;
    let root = repo.path();
    let manifest = root.join("crates/nested/Cargo.toml");
    let mut text = fs::read_to_string(&manifest)?;
    text.push_str("\n[lib]\npath = \"../shared/lib.rs\"\n");
    fs::write(&manifest, text)?;
    fs::create_dir_all(root.join("crates/shared"))?;
    fs::write(root.join("crates/shared/lib.rs"), "pub fn shared() {}\n")?;
    let jobs = finalized_jobs(&prepare(root)?)?;
    let names = snapshot_names(&jobs);
    let base = state_snapshots(&jobs, root)?;
    for (file, content, expected) in [
        (
            "crates/shared/lib.rs",
            "pub fn shared() { let _ = 1; }\n",
            vec!["crates/nested"],
        ),
        ("docs/guide.md", "words\n", vec![]),
    ] {
        let after = edit_and_measure(&jobs, root, file, content)?;
        let got = moved_roots(&names, &base, &after);
        if got != expected {
            return Err(format!("editing {file} moved {got:?}, wanted {expected:?}").into());
        }
    }
    Ok(())
}
