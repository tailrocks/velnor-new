// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;
use velnor_model::{EventKind, RepoPath, Target, TargetKind};
use velnor_rust::{LOCK_PATH, detect, lint_runs};

use crate::facts::TestResult;
use crate::generated_check_source::{
    FIRST, FROM_SOURCE, bin, check_step, job, locked, private_generator, rendered,
};
use crate::lint_lane::{LINT, MISE};
use crate::lock::{COMMIT_FILE, SUMS};
use crate::pipeline::workflow;

const GATED: &str = "[[lint]]\nkind = 'generated-check'\nfrom_source = true\ngate_on_plan = true\n";
const PLAIN_GATED: &str = "[[lint]]\nkind = 'generated-check'\ngate_on_plan = true\n";

fn run(job: &Value) -> String {
    check_step(job)["run"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn selected(config: &str, paths: &[&str]) -> Result<bool, Box<dyn std::error::Error>> {
    let mut repo = locked(&format!("{LINT}\n{config}"), FIRST)?;
    crate::facts::lock_pinned_tools(&mut repo)?;
    let facts = detect(&repo)?;
    let changed: BTreeSet<RepoPath> = paths
        .iter()
        .map(|path| RepoPath::new(*path))
        .collect::<Result<_, _>>()?;
    lint_runs(
        &facts,
        EventKind::PullRequest,
        Some(&changed),
        &BTreeSet::new(),
        &BTreeSet::new(),
    )
    .into_iter()
    .find(|(slug, _)| slug.as_str() == "generated_check")
    .map(|(_, run)| run)
    .ok_or_else(|| "no plan decision for the generated check".into())
}

fn step_named(job: &Value, name: &str) -> bool {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|step| step["name"].as_str() == Some(name))
}

#[test]
fn the_source_build_is_one_command_with_no_shell_logic() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    let step = check_step(&job);
    let run = run(&job);
    for logic in ["eval", "$(", "`", ";", "&&", "||", "|", "{", "if ", "fail"] {
        assert!(!run.contains(logic), "{logic}: {run}");
    }
    assert!(step["env"].is_null(), "{step}");
    Ok(())
}

#[test]
fn the_source_build_runs_through_the_velnor_ci_wrapper_like_every_cargo_step() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    assert!(
        run(&job).starts_with(r#". "$VELNOR_EXEC" run -- cargo run --locked "#),
        "{}",
        run(&job)
    );
    Ok(())
}

#[test]
fn the_source_build_stays_offline_after_the_private_fetch_revoked_the_token() -> TestResult {
    let job = job(&workflow(&private_generator()?)?)?;
    let step = check_step(&job);
    assert!(run(&job).contains("--locked --offline"), "{job}");
    assert_eq!(step["env"]["CARGO_NET_OFFLINE"], "true", "{job}");
    let plain = self::job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    assert!(!run(&plain).contains("--offline"), "{plain}");
    assert!(check_step(&plain)["env"].is_null(), "{plain}");
    Ok(())
}

#[test]
fn the_source_build_reports_its_cache_telemetry_and_the_cache_check_waits_for_it() -> TestResult {
    let text = rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?;
    let lint = job(&text)?;
    assert!(step_named(&lint, "Report cache telemetry"), "{lint}");
    assert!(step_named(&lint, "Upload cache metrics"), "{lint}");
    let cache = crate::yaml_tree::document(&text)?["jobs"]["cache"].clone();
    let needs = cache["needs"].to_string();
    assert!(needs.contains("lint-generated-check"), "{needs}");
    let released = rendered(
        &format!("{LINT}\n[[lint]]\nkind = 'generated-check'\n"),
        FIRST,
    )?;
    assert!(!step_named(&job(&released)?, "Report cache telemetry"));
    Ok(())
}

#[test]
fn the_source_build_logs_cargo_fingerprints_when_cache_debug_is_on() -> TestResult {
    let config = format!("cache_debug = true\n{LINT}\n{FROM_SOURCE}");
    let job = job(&rendered(&config, FIRST)?)?;
    let log = check_step(&job)["env"]["CARGO_LOG"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(log.contains("fingerprint"), "{job}");
    assert!(
        step_named(&job, "Upload the cargo fingerprint log"),
        "{job}"
    );
    let off = self::job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    assert!(check_step(&off)["env"].is_null(), "{off}");
    assert!(
        !step_named(&off, "Upload the cargo fingerprint log"),
        "{off}"
    );
    Ok(())
}

#[test]
fn from_source_refuses_a_velnor_gen_binary_that_needs_features() -> TestResult {
    let mut repo = locked(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?;
    let gated = Target {
        requires_features: true,
        ..bin("velnor-gen")?
    };
    repo.rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no root")?
        .members[0]
        .targets = vec![gated];
    let text = format!("{:?}", workflow(&repo).err());
    assert!(text.contains("GeneratorPackage"), "{text}");
    let ungated = Target {
        kinds: vec![TargetKind::Bin],
        ..bin("velnor-gen")?
    };
    repo.rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no root")?
        .members[0]
        .targets = vec![ungated];
    assert!(workflow(&repo).is_ok());
    Ok(())
}

#[test]
fn from_source_needs_a_rust_that_knows_the_host_tuple_target() -> TestResult {
    for (version, refused) in [("1.90.0", true), ("1.91.0", false)] {
        let mut repo = locked(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?;
        repo.files.insert(
            RepoPath::new("mise.toml")?,
            MISE.replace("1.99.1", version).into_bytes(),
        );
        let text = format!("{:?}", workflow(&repo).err());
        assert_eq!(
            text.contains("GeneratorHostTuple"),
            refused,
            "{version}: {text}"
        );
    }
    Ok(())
}

#[test]
fn a_gated_source_build_runs_for_markdown_the_generator_may_compile_in() -> TestResult {
    let paths = ["crates/velnor-gen/README.md"];
    assert!(selected(GATED, &paths)?);
    assert!(!selected(PLAIN_GATED, &paths)?);
    Ok(())
}

#[test]
fn from_source_still_needs_the_generator_digests_that_verify_release_compares() -> TestResult {
    let mut repo = locked(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?;
    let raw = velnor_rust::pin_lock(FIRST, COMMIT_FILE, SUMS, None)?;
    repo.files.insert(LOCK_PATH, raw.into_bytes());
    let text = format!("{:?}", workflow(&repo).err());
    assert!(text.contains("GeneratorNotLocked"), "{text}");
    Ok(())
}

#[test]
fn from_source_wires_the_shared_mbx_store_before_the_tools_install() -> TestResult {
    let config = format!("mbx_shared_store = true\n{LINT}\n{FROM_SOURCE}");
    let job = job(&rendered(&config, FIRST)?)?;
    let names: Vec<String> = job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["name"].as_str().map(str::to_owned))
        .collect();
    let at = |name: &str| names.iter().position(|found| found == name);
    let store = at("velnor-ci mbx-store").ok_or("no shared store step")?;
    let tools = at("Install tools").ok_or("no tools step")?;
    assert!(store < tools, "{names:?}");
    assert!(at("Private mbx store").is_none(), "{names:?}");
    Ok(())
}

#[test]
fn from_source_keeps_the_date_of_a_host_qualified_nightly() -> TestResult {
    for (version, refused) in [
        ("nightly-2025-01-01-x86_64-unknown-linux-gnu", true),
        ("nightly-2025-01-01-aarch64-unknown-linux-gnu", true),
        ("nightly-2025-10-01-x86_64-unknown-linux-gnu", false),
        ("nightly-2025-10-01-aarch64-unknown-linux-gnu", false),
    ] {
        let mut repo = locked(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?;
        repo.files.insert(
            RepoPath::new("mise.toml")?,
            MISE.replace("1.99.1", version).into_bytes(),
        );
        let text = format!("{:?}", workflow(&repo).err());
        assert_eq!(
            text.contains("GeneratorHostTuple"),
            refused,
            "{version}: {text}"
        );
    }
    Ok(())
}

#[test]
fn a_nested_generator_builds_into_its_detected_target_directory() -> TestResult {
    let roots = [
        ("Cargo.toml", "velnor-ci-app"),
        ("tools/gen/Cargo.toml", "velnor-gen"),
    ];
    let repo =
        crate::generated_check_source::packages(&format!("{LINT}\n{FROM_SOURCE}"), FIRST, &roots)?;
    let job = job(&workflow(&repo)?)?;
    let target = check_step(&job)["env"]["CARGO_TARGET_DIR"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        target.contains("target/velnor-roots/") && !target.contains("tools/gen"),
        "{target}: {job}"
    );
    Ok(())
}
