// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, os::unix::fs::PermissionsExt};

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::one_job_per_crate::mbx_repo_for;

const BUILD_STEP: &str = "Build ledger CLI";
const PUBLISH_STEP: &str = "Publish ledger CLI outputs";
const LEAN_PROFILE: [(&str, &str); 3] = [
    ("CARGO_PROFILE_DEV_DEBUG", "0"),
    ("CARGO_INCREMENTAL", "0"),
    ("CARGO_PROFILE_DEV_STRIP", "symbols"),
];

struct StepText {
    env: Vec<(String, String)>,
    run: String,
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn unquote(value: &str) -> Result<String, Box<dyn Error>> {
    if value.starts_with('"') {
        Ok(serde_json::from_str(value)?)
    } else {
        Ok(value.to_owned())
    }
}

fn nested(lines: &[&str], from: usize) -> Vec<String> {
    let parent = indent(lines[from]);
    lines[from + 1..]
        .iter()
        .take_while(|line| indent(line) > parent)
        .map(|line| line.trim().to_owned())
        .collect()
}

fn env_entries(
    lines: &[&str],
    env_at: usize,
    step: &str,
) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let header = lines[env_at].trim();
    let anchor_use = header.strip_prefix("env: *");
    let block_at = match anchor_use {
        Some(alias) => {
            let definition = format!("env: &{alias}");
            lines
                .iter()
                .position(|line| line.trim() == definition)
                .ok_or_else(|| format!("{step}: env alias *{alias} has no definition"))?
        }
        None => env_at,
    };
    nested(lines, block_at)
        .iter()
        .map(|entry| {
            let (key, value) = entry
                .split_once(": ")
                .ok_or_else(|| format!("{step}: env line {entry:?} is not key: value"))?;
            Ok((key.to_owned(), unquote(value)?))
        })
        .collect()
}

fn step_text(text: &str, name: &str) -> Result<StepText, Box<dyn Error>> {
    let lines: Vec<&str> = text.lines().collect();
    let marker = format!("name: {name}");
    let named: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim().trim_start_matches("- ") == marker)
        .map(|(at, _)| at)
        .collect();
    let [start] = named[..] else {
        return Err(format!("{name}: expected one step, found {}", named.len()).into());
    };
    let floor = indent(lines[start]);
    let body: Vec<&str> = lines[start + 1..]
        .iter()
        .copied()
        .take_while(|line| {
            indent(line) > floor || (indent(line) == floor && !line.trim().starts_with("- "))
        })
        .collect();
    let env_at = body.iter().position(|line| line.trim().starts_with("env:"));
    let run = body
        .iter()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .ok_or_else(|| format!("{name}: step has no run"))?;
    Ok(StepText {
        env: env_at.map_or_else(|| Ok(Vec::new()), |at| env_entries(&body, at, name))?,
        run: unquote(run)?,
    })
}

fn render_tree_text(policy: WorkflowPolicy) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let repo = mbx_repo_for(false, policy)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(tree
        .files
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect())
}

fn occurrences(tree: &[(String, String)], key: &str) -> usize {
    tree.iter()
        .map(|(_, bytes)| bytes.matches(key).count())
        .sum()
}

fn env_values<'a>(env: &'a [(String, String)], key: &str) -> Vec<&'a str> {
    env.iter()
        .filter(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .collect()
}

#[cfg(unix)]
fn native_image_repository() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = mbx_repo_for(false, WorkflowPolicy::VelnorRepositoryV1)?;
    let config_path = repo.path().join(".velnor/config.toml");
    let mut config = std::fs::read_to_string(&config_path)?;
    let checkout = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repository_config = std::fs::read_to_string(checkout.join(".velnor/config.toml"))?;
    let image_blocks = repository_config
        .split_once("[[images]]")
        .ok_or("images")?
        .1
        .split_once("\n[[checks]]")
        .ok_or("image end")?
        .0;
    config.push_str("\n[[images]]");
    config.push_str(image_blocks);
    let nested_manifest = repo.path().join("crates/velnor-runner/Cargo.toml");
    std::fs::create_dir_all(nested_manifest.parent().ok_or("manifest parent")?)?;
    std::fs::write(
        nested_manifest,
        "[workspace]\nmembers = []\n[workspace.package]\nversion = \"0.1.0\"\n",
    )?;
    config.push_str("\n[[checks]]\nid = \"native-image-candidates\"\ntask = \"fixture-image-check\"\ninputs = [\"Cargo.toml\"]\ntools = []\nrunner = { label = \"ubuntu-26.04\", platform = \"linux_x64\", executor = \"hosted\" }\n");
    std::fs::write(config_path, config)?;
    let mise = repo.path().join("mise.toml");
    let mut task = std::fs::read_to_string(&mise).unwrap_or_default();
    task.push_str("\n[tasks.fixture-image-check]\nrun = \"true\"\n");
    std::fs::write(mise, task)?;
    Ok(repo)
}

fn file_text<'a>(files: &'a [(String, String)], path: &str) -> Result<&'a str, Box<dyn Error>> {
    files
        .iter()
        .find(|(file, _)| file == path)
        .map(|(_, bytes)| bytes.as_str())
        .ok_or_else(|| format!("missing {path}").into())
}

fn assert_publish_authority(files: &[(String, String)]) -> Result<(), Box<dyn Error>> {
    let publish = file_text(files, ".github/workflows/publish.yml")?;
    let authority = publish
        .split_once("publish-authority:")
        .ok_or("publish authority")?
        .1
        .split_once("outputs:")
        .ok_or("authority outputs")?
        .1
        .split_once("steps:")
        .ok_or("authority steps")?
        .0;
    assert!(authority.contains("artifact:"));
    assert!(!authority.contains("version:"));
    Ok(())
}

fn assert_native_job_shape(files: &[(String, String)]) -> Result<String, Box<dyn Error>> {
    let ci = file_text(files, WORKFLOW_PATH)?;
    for image in ["dind", "job", "resource-probe", "runner"] {
        assert!(!ci.contains(&format!("image-{image}-amd64:")));
    }
    let job = ci
        .split_once("check-native-image-candidates:")
        .ok_or("native check job")?
        .1;
    let initial = job
        .find("name: Verify Rust channel manifest")
        .ok_or("initial verifier")?;
    let install = job
        .find("name: Install pinned Linux musl target")
        .ok_or("target install")?;
    let post_name = "Verify Rust toolchain after cross-target install";
    let post = job
        .find(&format!("name: {post_name}"))
        .ok_or("post-target verifier")?;
    let build = job
        .find("name: Build locked resource probe")
        .ok_or("probe build")?;
    assert!(initial < install && install < post && post < build);
    assert_eq!(
        files
            .iter()
            .map(|(_, bytes)| bytes.matches(&format!("name: {post_name}")).count())
            .sum::<usize>(),
        1
    );
    assert!(
        files
            .iter()
            .filter(|(_, bytes)| bytes.contains("tools-producer-"))
            .any(|(_, bytes)| { bytes.contains("./.github/actions/velnor-verify-rust-manifest") })
    );
    let after_target = step_text(job, post_name)?;
    assert!(after_target.run.contains("rust_toolchain_bytes_mismatch"));
    assert!(after_target.run.contains("channel-rust-"));
    assert_eq!(
        env_values(&after_target.env, "RUSTUP_HOME"),
        ["${{ runner.temp }}/velnor/rustup"]
    );
    assert_eq!(
        env_values(&after_target.env, "CARGO_HOME"),
        ["${{ runner.temp }}/velnor/cargo"]
    );
    let acquire = step_text(job, "Acquire approved check Mise")?;
    for expected in [
        "2026.10.19",
        "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d",
        "173449632",
    ] {
        assert!(
            acquire.run.contains(expected),
            "missing approved runtime {expected}"
        );
    }
    let execute = step_text(job, "Execute named check")?;
    assert_eq!(
        env_values(&execute.env, "PATH"),
        ["${{ runner.temp }}/velnor/check-mise/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"]
    );
    for step in [
        "Build exact-source image maintenance",
        "Build locked resource probe",
        "Build and inspect CI image candidates",
    ] {
        let body = step_text(job, step)?;
        assert!(
            body.run
                .contains("scripts/ci-native-image-credential-exec.sh"),
            "{step}: {}",
            body.run
        );
    }
    Ok(job.to_owned())
}

#[cfg(unix)]
fn assert_credentials_scrubbed(job: &str) -> Result<(), Box<dyn Error>> {
    let tools = tempfile::tempdir()?;
    let fake_mise = tools.path().join("mise");
    std::fs::write(&fake_mise, "#!/bin/sh\nenv\n")?;
    std::fs::set_permissions(&fake_mise, std::fs::Permissions::from_mode(0o755))?;
    let probe = step_text(job, "Build locked resource probe")?;
    assert_eq!(env_values(&probe.env, "PATH").len(), 0);
    let observed = std::process::Command::new("bash")
        .args(["-c", &probe.run])
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .env("PATH", format!("{}:/usr/bin:/bin", tools.path().display()))
        .env("GITHUB_TOKEN", "sensitive-token")
        .env("UNEXPECTED_SECRET", "sensitive-secret")
        .env("TEST_PASSWORD", "sensitive-password")
        .env("ACTIONS_ID_TOKEN_REQUEST_URL", "sensitive-url")
        .output()?;
    assert!(observed.status.success());
    let output = String::from_utf8(observed.stdout)?;
    assert!(
        !output.contains("sensitive-"),
        "credential reached build: {output}"
    );
    for artifact in ["velnor-job-image-candidate-", "image-assets-"] {
        let line = job
            .lines()
            .position(|line| line.contains(artifact))
            .ok_or("artifact")?;
        let lines = job.lines().collect::<Vec<_>>();
        let before = lines[line.saturating_sub(12)..line].join("\n");
        assert!(
            before.contains("success()")
                && before.contains("refs/heads/main")
                && before.contains("push"),
            "{artifact}: {before}"
        );
    }
    Ok(())
}

#[test]
fn ledger_cli_build_uses_the_lean_dev_profile() -> Result<(), Box<dyn Error>> {
    let tree = render_tree_text(WorkflowPolicy::VelnorRepositoryV1)?;
    let workflow = tree
        .iter()
        .find(|(path, _)| path == WORKFLOW_PATH)
        .map(|(_, bytes)| bytes.as_str())
        .ok_or("no ci workflow")?;
    let build = step_text(workflow, BUILD_STEP)?;
    for (key, value) in LEAN_PROFILE {
        assert_eq!(env_values(&build.env, key), [value], "{key}");
    }
    assert_eq!(
        env_values(&build.env, "CARGO_TARGET_DIR"),
        ["${{ runner.temp }}/velnor/ledger-cli-target"]
    );
    assert_eq!(env_values(&build.env, "CARGO_HOME").len(), 1);
    assert!(build.run.contains(" --locked "), "{}", build.run);
    assert!(build.run.contains(" --offline "), "{}", build.run);
    let publish = step_text(workflow, PUBLISH_STEP)?;
    for (key, _) in LEAN_PROFILE {
        assert!(env_values(&publish.env, key).is_empty(), "{key} on publish");
    }
    Ok(())
}

#[test]
fn lean_dev_profile_stays_on_the_ledger_cli_build() -> Result<(), Box<dyn Error>> {
    let velnor = render_tree_text(WorkflowPolicy::VelnorRepositoryV1)?;
    let workflow = velnor
        .iter()
        .find(|(path, _)| path == WORKFLOW_PATH)
        .map(|(_, bytes)| bytes.as_str())
        .ok_or("no ci workflow")?;
    let build = step_text(workflow, BUILD_STEP)?;
    for (key, _) in LEAN_PROFILE {
        assert_eq!(occurrences(&velnor, key), 1, "{key} in velnor tree");
        assert_eq!(env_values(&build.env, key).len(), 1, "{key} on build step");
    }
    let consumer = render_tree_text(WorkflowPolicy::ConsumerV1)?;
    for (key, _) in LEAN_PROFILE {
        assert_eq!(occurrences(&consumer, key), 0, "{key} in consumer tree");
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn native_image_ci_producer_scrubs_credentials_and_uploads_only_on_success()
-> Result<(), Box<dyn Error>> {
    let repo = native_image_repository()?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let files: Vec<_> = tree
        .files
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect();
    assert_publish_authority(&files)?;
    let job = assert_native_job_shape(&files)?;
    assert_credentials_scrubbed(&job)?;
    Ok(())
}
