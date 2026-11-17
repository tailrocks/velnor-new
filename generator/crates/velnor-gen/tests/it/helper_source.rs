// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_files;
use velnor_model::{RepoPath, RepoView, TargetKind};
use velnor_rust::{CONFIG_PATH, DetectError, Reason};

use crate::facts::{TestResult, package, path, repo};
use crate::golden::{load, samples};
use crate::pipeline::{view, workflow};

const HELPER_LOCK: &[u8] =
    include_bytes!("../../../../samples/helper-lock/view/.velnor/rust-ci.lock");

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
fn the_helper_build_and_upload_require_an_artifact_consumer() -> TestResult {
    let text = text_of(&sample(Some("helper = { path = \"ci-helper/\" }\nskip_draft_pull_requests = true\n"))?)?;
    let helper = text.split("  rust-helper:\n").nth(1).ok_or("no helper")?
        .split("\n  rust-root:").next().ok_or("no helper body")?;
    assert!(helper.contains("needs: rust-plan"), "{helper}");
    for name in ["Build velnor-ci", "Upload velnor-ci"] {
        let step = helper.split("      - ").find(|step| step.contains(&format!("name: {name}"))).ok_or("no helper step")?;
        for clause in ["rust_root_mode != ''none''", "rust_ci_helper_mode != ''none''", "rust_deny == ''run''", "rust_machete == ''run''", "github.event_name == ''schedule''"] {
            assert!(step.contains(clause), "{name}: {step}");
        }
        assert!(step.contains(" || "), "{step}");
    }
    assert_eq!(text.matches("cargo build -p velnor-ci --locked").count(), 1);
    assert_eq!(text.matches("name: Upload velnor-ci").count(), 1);
    Ok(())
}

#[test]
fn the_helper_key_builds_the_helper_once_and_every_lane_downloads_it() -> TestResult {
    let text = text_of(&sample(None)?)?;
    let jobs = job_ids(&text);
    assert_eq!(jobs[..2], ["rust-plan", "rust-helper"]);
    assert_eq!(jobs.iter().filter(|job| **job == "rust-helper").count(), 1);
    let lanes: Vec<&&str> = jobs
        .iter()
        .filter(|job| job.starts_with("rust-") && !matches!(**job, "rust-plan" | "rust-helper"))
        .collect();
    assert_eq!(lanes.len(), 6);
    assert_eq!(text.matches("Install velnor-ci").count(), 2);
    assert_eq!(text.matches("Download velnor-ci").count(), lanes.len());
    assert_eq!(text.matches("cargo build -p velnor-ci --locked").count(), 1);
    for lane in lanes {
        let header = format!("  {lane}:\n    needs:\n      - rust-plan\n      - rust-helper\n");
        assert!(text.contains(&header), "{lane}");
    }
    assert!(
        text.contains("    needs:\n      - rust-plan\n      - rust-helper\n      - rust-root\n")
    );
    Ok(())
}

#[test]
fn the_plan_job_and_the_helper_job_use_the_released_helper() -> TestResult {
    let text = text_of(&sample(None)?)?;
    for job in ["rust-plan", "rust-helper"] {
        let start = text.find(&format!("  {job}:\n")).ok_or("job missing")?;
        let body = &text[start..];
        let end = body[1..].find("\n  rust-").map_or(body.len(), |at| at + 1);
        let body = &body[..end];
        assert!(body.contains("Install velnor-ci 0.1.79"), "{job}");
        assert!(!body.contains("Download velnor-ci"), "{job}");
    }
    let verdict = &text[text.find("  required:").ok_or("no verdict")?..];
    assert!(!verdict.contains("velnor-ci"));
    Ok(())
}

#[test]
fn the_job_that_builds_the_helper_uploads_the_fingerprint_log_too() -> TestResult {
    let text = text_of(&sample(None)?)?;
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
    view.files
        .insert(path(".velnor/rust-ci.lock")?, HELPER_LOCK.to_vec());
    view.tracked_paths.insert(CONFIG_PATH);
    view.files.insert(
        CONFIG_PATH,
        b"helper = { path = \"ci-helper/\" }\n".to_vec(),
    );
    let text = text_of(&view)?;
    assert_eq!(
        job_ids(&text),
        ["rust-plan", "rust-helper", "rust", "required"]
    );
    assert_eq!(text.matches("Install velnor-ci").count(), 2);
    assert_eq!(text.matches("Download velnor-ci").count(), 1);
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

fn helper_path(config: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    let mut view = sample(None)?;
    if let Some(config) = config {
        let file = path("ci-helper/.cargo/config.toml")?;
        view.tracked_paths.insert(file.clone());
        view.files.insert(file, config.as_bytes().to_vec());
    }
    let text = text_of(&view)?;
    text.lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("path: ")
                .filter(|path| path.contains("velnor-ci"))
        })
        .map(str::to_owned)
        .ok_or_else(|| "no upload path".into())
}

fn helper_with_configs(files: &[(&str, &str)]) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = sample(None)?;
    for (name, config) in files {
        let file = path(name)?;
        view.tracked_paths.insert(file.clone());
        view.files.insert(file, config.as_bytes().to_vec());
    }
    Ok(view)
}

#[test]
fn the_upload_path_follows_the_repository_target_dir() -> TestResult {
    assert_eq!(helper_path(None)?, "ci-helper/target/debug/velnor-ci");
    assert_eq!(
        helper_path(Some("[build]\ntarget-dir = \"out\"\n"))?,
        "ci-helper/out/debug/velnor-ci"
    );
    Ok(())
}

#[test]
fn a_build_target_in_any_config_of_the_helper_root_refuses_the_helper() -> TestResult {
    let triple = "[build]\ntarget = \"aarch64-unknown-linux-gnu\"\n";
    let array = "[build]\ntarget = [\"aarch64-unknown-linux-gnu\"]\n";
    for files in [
        vec![(".cargo/config.toml", triple)],
        vec![("ci-helper/.cargo/config.toml", triple)],
        vec![("ci-helper/.cargo/config.toml", array)],
        vec![
            (".cargo/config.toml", array),
            (
                "ci-helper/.cargo/config.toml",
                "[build]\ntarget-dir = \"out\"\n",
            ),
        ],
    ] {
        let error = expected_files(&helper_with_configs(&files)?)
            .err()
            .ok_or("accepted")?;
        assert!(
            matches!(
                error,
                velnor_gen::GenerateError::Detect(DetectError::Invalid {
                    reason: Reason::HelperTarget(_),
                    ..
                })
            ),
            "{files:?}: {error:?}"
        );
    }
    Ok(())
}

#[test]
fn the_helper_upload_includes_hidden_files_so_a_dot_target_dir_uploads() -> TestResult {
    let text = text_of(&helper_with_configs(&[(
        "ci-helper/.cargo/config.toml",
        "[build]\ntarget-dir = \".target\"\n",
    )])?)?;
    assert!(text.contains("path: ci-helper/.target/debug/velnor-ci"));
    let upload = text.split("Upload velnor-ci").nth(1).ok_or("no upload")?;
    let upload = upload.split("- name:").next().unwrap_or_default();
    assert!(upload.contains("include-hidden-files: true"), "{upload}");
    Ok(())
}

#[test]
fn a_target_dir_with_glob_characters_is_uploaded_literally() -> TestResult {
    for (dir, literal) in [
        ("o*t", "ci-helper/o[*]t/debug/velnor-ci"),
        ("o?t", "ci-helper/o[?]t/debug/velnor-ci"),
        ("o[t]", "ci-helper/o[[]t]/debug/velnor-ci"),
    ] {
        let config = format!("[build]\ntarget-dir = \"{dir}\"\n");
        assert_eq!(helper_path(Some(&config))?, format!("'{literal}'"));
    }
    Ok(())
}
