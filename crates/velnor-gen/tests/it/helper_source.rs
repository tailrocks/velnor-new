// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_files;
use velnor_model::{RepoPath, RepoView, TargetKind};
use velnor_rust::{CONFIG_PATH, DetectError, LOCK_PATH, Reason};

use crate::facts::{TestResult, package, path, repo};
use crate::golden::{load, samples};
use crate::pipeline::{view, workflow};

const HELPER_LOCK: &[u8] =
    include_bytes!("../../../../generator/samples/helper-source/view/.velnor/rust-ci.lock");

const HELPER_CONFIG: &str = "exclude = [\"fixtures/**\"]\nhelper = { path = \"ci-helper\" }\n";

fn sample(config: Option<&str>) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = load(&samples().join("helper-source"))?;
    if let Some(config) = config {
        view.files.insert(CONFIG_PATH, config.as_bytes().to_vec());
    }
    Ok(view)
}

fn text_of(view: &RepoView) -> Result<String, Box<dyn std::error::Error>> {
    expected_files(view)?
        .remove(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or_else(|| "no workflow".into())
}

fn job_ids(text: &str) -> Vec<&str> {
    text.lines()
        .skip_while(|line| *line != "jobs:")
        .filter(|line| line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        .map(|line| line.trim().trim_end_matches(':'))
        .collect()
}

#[test]
fn the_helper_key_builds_the_helper_once_without_making_checks_depend_on_it() -> TestResult {
    let text = text_of(&sample(None)?)?;
    let jobs = job_ids(&text);
    assert_eq!(jobs[..2], ["rust-plan", "rust-helper"]);
    assert_eq!(jobs.iter().filter(|job| **job == "rust-helper").count(), 1);
    let helper = text
        .split("  rust-helper:\n")
        .nth(1)
        .ok_or("helper job missing")?
        .split("\n  rust-")
        .next()
        .ok_or("helper job body missing")?;
    assert!(helper.starts_with("    needs:\n      - rust-plan\n    if:"));
    assert_eq!(
        helper.lines().find(|line| line.starts_with("    if:")),
        Some(
            "    if: '${{ (github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed'')) && (needs.rust-plan.result == ''success'' && needs.rust-plan.outputs.rust_ci_helper_mode != ''none'') }}'"
        )
    );
    let lanes: Vec<&&str> = jobs
        .iter()
        .filter(|job| job.starts_with("rust-") && !matches!(**job, "rust-plan" | "rust-helper"))
        .collect();
    assert_eq!(lanes.len(), 7);
    assert_eq!(
        text.matches("cargo build -p velnor-ci --bin velnor-ci --locked")
            .count(),
        1
    );
    assert!(!text.contains("Upload velnor-ci"));
    for lane in lanes.into_iter().filter(|lane| **lane != "rust-trim") {
        let header = format!("  {lane}:\n    needs:\n      - rust-plan\n");
        let start = text.find(&header).ok_or(lane.to_string())?;
        let needs = text[start..]
            .split("    if:")
            .next()
            .ok_or("no condition")?;
        assert!(!needs.contains("rust-helper"), "{lane}: {needs}");
    }
    assert!(
        text.contains("    needs:\n      - rust-plan\n      - rust-helper\n      - rust-root\n")
    );
    Ok(())
}

#[test]
fn every_check_job_uses_the_released_helper_even_when_building_it_from_source() -> TestResult {
    let mut view = sample(None)?;
    let mise = path("mise.toml")?;
    let tools = String::from_utf8(view.files.get(&mise).ok_or("mise missing")?.clone())?;
    view.files.insert(
        mise,
        tools
            .replace("[tools]\n", "[tools]\ncargo-machete = \"0.9.1\"\n")
            .into_bytes(),
    );
    view.files.remove(&path("mise.lock")?);
    crate::facts::lock_pinned_tools(&mut view)?;
    let text = text_of(&view)?;
    assert!(job_ids(&text).contains(&"policy"));
    let jobs = job_ids(&text);
    assert_eq!(text.matches("Install velnor-ci 0.1.81").count(), jobs.len());
    for (index, job) in jobs.iter().enumerate() {
        let start = text.find(&format!("  {job}:\n")).ok_or("job missing")?;
        let end = jobs.get(index + 1).map_or(Ok(text.len()), |next| {
            text.find(&format!("  {next}:\n")).ok_or("next job missing")
        })?;
        let body = &text[start..end];
        assert!(body.contains("Install velnor-ci 0.1.81"), "{job}");
        if matches!(*job, "rust-root" | "rust-ci_helper" | "rust-inputs-audit") {
            let restore = body
                .find("velnor-ci inputs restore")
                .ok_or("no inputs restore")?;
            let restored = body.find("velnor-ci restored").ok_or("no restored")?;
            assert!(restore < restored, "{job}");
        }
    }
    for forbidden in ["Download velnor-ci", "Make velnor-ci executable", "vbin"] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
    let source_lane = text
        .split("  rust-ci_helper:\n")
        .nth(1)
        .ok_or("source lane missing")?;
    let source_lane = source_lane
        .split("\n  rust-")
        .next()
        .ok_or("source lane body missing")?;
    for command in ["cargo-nextest nextest run", "cargo-clippy clippy"] {
        let step = source_lane
            .split("- name:")
            .find(|step| step.contains(command))
            .ok_or("source check missing")?;
        assert!(step.contains("working-directory: ci-helper"), "{step}");
    }
    let verdict = &text[text.find("  required:").ok_or("no verdict")?
        ..text.find("\n  cache:\n").ok_or("no Cache")?];
    assert!(verdict.contains("Install velnor-ci 0.1.81"));
    assert!(verdict.contains("\"$VELNOR_EXEC\" gate --spec-env GATE_SPEC"));
    assert!(verdict.contains("--job rust-helper=lane:ci_helper"));
    assert!(!verdict.contains("Check out"));
    assert!(!verdict.contains("download-artifact"));
    Ok(())
}

#[test]
fn the_job_that_builds_the_helper_uploads_the_fingerprint_log_too() -> TestResult {
    let mut repo = sample(Some(&format!("{HELPER_CONFIG}cache_debug = true\n")))?;
    let lock = String::from_utf8(repo.files.get(&LOCK_PATH).ok_or("no lock")?.clone())?;
    repo.files
        .insert(LOCK_PATH, lock.replace("0.1.81", "0.1.82").into_bytes());
    let text = text_of(&repo)?;
    let start = text.find("  rust-helper:\n").ok_or("no helper job")?;
    let body = &text[start..];
    let end = body[1..].find("\n  rust-").map_or(body.len(), |at| at + 1);
    assert!(
        body[..end].contains("Upload the cargo fingerprint log"),
        "{}",
        &body[..end]
    );
    Ok(())
}

#[test]
fn without_the_helper_key_no_job_builds_or_downloads_the_helper() -> TestResult {
    let text = text_of(&sample(Some("exclude = [\"fixtures/**\"]\n"))?)?;
    assert!(!job_ids(&text).contains(&"rust-helper"));
    assert!(!text.contains("Download velnor-ci"));
    assert!(!text.contains("Upload velnor-ci"));
    assert!(!text.contains("cargo build -p"));
    assert!(text.matches("Install velnor-ci").count() > 2);
    Ok(())
}

#[test]
fn a_single_root_with_the_helper_key_still_plans_in_a_job_with_the_released_helper() -> TestResult {
    let helper = package(
        "velnor-ci",
        "ci-helper/Cargo.toml",
        &[],
        &[("velnor-ci", TargetKind::Bin, true)],
    )?;
    let mut view = repo(
        vec![("ci-helper/Cargo.toml", vec![helper])],
        &[(
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
        )],
    )?;
    view.tracked_paths.insert(path(".velnor/rust-ci.lock")?);
    view.files.insert(
        path(".velnor/rust-ci.lock")?,
        String::from_utf8(HELPER_LOCK.to_vec())?.into_bytes(),
    );
    view.tracked_paths.insert(CONFIG_PATH);
    view.files.insert(
        CONFIG_PATH,
        b"helper = { path = \"ci-helper/\" }\n".to_vec(),
    );
    let text = text_of(&view)?;
    assert_eq!(
        job_ids(&text),
        [
            "rust-plan",
            "rust-helper",
            "rust",
            "rust-trim",
            "required",
            "cache"
        ]
    );
    assert_eq!(text.matches("Install velnor-ci").count(), 6);
    let verdict = &text[text.find("  required:").ok_or("no verdict")?..];
    let verdict = verdict.split("\n  cache:").next().unwrap_or(verdict);
    assert!(verdict.contains("Install velnor-ci 0.1.81"));
    assert!(verdict.contains("\"$VELNOR_EXEC\" gate --spec-env GATE_SPEC"));
    assert!(verdict.contains("--job rust-helper=lane:ci_helper"));
    assert!(!verdict.contains("Check out"));
    assert!(!verdict.contains("download-artifact"));
    let helper = text
        .split("  rust-helper:\n")
        .nth(1)
        .ok_or("helper job missing")?
        .split("\n  rust:")
        .next()
        .ok_or("helper job body missing")?;
    assert!(helper.starts_with("    needs:\n      - rust-plan\n    if:"));
    assert_eq!(
        helper.lines().find(|line| line.starts_with("    if:")),
        Some(
            "    if: '${{ (github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed'')) && (needs.rust-plan.result == ''success'' && needs.rust-plan.outputs.rust_ci_helper_mode != ''none'') }}'"
        )
    );
    assert!(!text.contains("Download velnor-ci"));
    assert!(text.contains("cargo-clippy clippy"));
    assert!(text.contains("cargo-nextest nextest run"));
    Ok(())
}

#[test]
fn a_helper_path_that_is_not_a_cargo_root_with_velnor_ci_is_an_error_naming_it() -> TestResult {
    for config in [
        "helper = { path = \"nowhere\" }\n",
        "helper = { path = \"lab\" }\n",
        "helper = { path = \"\" }\n",
    ] {
        let rejected = workflow(&view(
            &["Cargo.toml", "lab/Cargo.toml"],
            Some(config.as_bytes()),
        )?);
        let message = rejected.err().ok_or("accepted")?.to_string();
        assert!(message.contains("rust-ci.toml"), "{config}: {message}");
    }
    let error = expected_files(&{
        let mut view = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
        view.tracked_paths.insert(CONFIG_PATH);
        view.files
            .insert(CONFIG_PATH, b"helper = { path = \"lab\" }\n".to_vec());
        view
    })
    .err()
    .ok_or("accepted")?;
    assert_eq!(
        error,
        velnor_gen::GenerateError::Detect(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::HelperBox(RepoPath::new("lab")?),
        })
    );
    Ok(())
}

#[test]
fn the_helper_key_takes_a_table_with_a_path_and_nothing_else() -> TestResult {
    for config in [
        "helper = \"ci-helper\"\n",
        "helper = { path = \"ci-helper\", release = \"1\" }\n",
        "helper = {}\n",
    ] {
        let mut broken = sample(None)?;
        broken.files.insert(CONFIG_PATH, config.as_bytes().to_vec());
        assert!(expected_files(&broken).is_err(), "{config}");
    }
    Ok(())
}

#[test]
fn helper_source_requires_a_pinned_helper() -> TestResult {
    let mut view = sample(None)?;
    let lock = path(".velnor/rust-ci.lock")?;
    let released = String::from_utf8(view.files.get(&lock).ok_or("no lock")?.clone())?;
    view.files.remove(&lock);
    let error = expected_files(&view)
        .err()
        .ok_or("accepted helper.path without a pinned helper")?;
    assert!(error.to_string().contains("pin-helper"), "{error}");
    view.files.insert(lock, released.into_bytes());
    assert!(expected_files(&view).is_ok());
    Ok(())
}
