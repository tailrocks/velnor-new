//! Binary-spawn smoke tests: init lifecycle, usage codes, env parity.

use std::error::Error;

use crate::impl_cli_tmp::git_fixture;
use crate::impl_cli_tmp::{
    add_crate_pair, cleanup, code, fresh_tempdir, git_init, init_repo, pin_branch, spawn,
};

#[test]
fn init_creates_config_then_refuses_overwrite() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-init")?;
    git_init(&tmp)?;
    let config = tmp.join(".velnor").join("config.toml");
    let first = spawn(&["init"], &[], &tmp)?;
    assert_eq!(code(&first), 0);
    assert!(config.is_file());
    let body = std::fs::read_to_string(&config)?;
    assert!(body.contains("schema = 1"));
    let second = spawn(&["init"], &[], &tmp)?;
    assert_eq!(code(&second), 1);
    assert_eq!(std::fs::read_to_string(&config)?, body);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn init_outside_work_tree_exits_one() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-norepo")?;
    let output = spawn(&["init"], &[], &tmp)?;
    assert_eq!(code(&output), 1);
    assert!(!tmp.join(".velnor").exists());
    cleanup(&tmp);
    Ok(())
}

#[test]
fn unknown_command_exits_two() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-unknown")?;
    let output = spawn(&["bogus"], &[], &tmp)?;
    assert_eq!(code(&output), 2);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn env_without_request_file_exits_two() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-env")?;
    git_init(&tmp)?;
    let missing = tmp.join("plan-v1-request.json");
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command
        .current_dir(&tmp)
        .env("VELNOR_INTERNAL_OP", "plan-v1")
        .env("VELNOR_REQUEST_FILE", &missing);
    let gated = command.output()?;
    assert_eq!(code(&gated), 2);
    let bare = spawn(&[], &[], &tmp)?;
    assert_eq!(gated.stdout, bare.stdout);
    assert_eq!(gated.stderr, bare.stderr);
    cleanup(&tmp);
    Ok(())
}

/// Recommendation bodies from the trailing plan-report section.
fn plan_recommendations(stdout: &str) -> Vec<String> {
    let mut in_section = false;
    let mut out = Vec::new();
    for line in stdout.lines() {
        if line == "Recommendations" {
            in_section = true;
        } else if in_section {
            if let Some(body) = line.strip_prefix("  ") {
                if body != "(none)" {
                    out.push(body.to_owned());
                }
            } else {
                break;
            }
        }
    }
    out
}

#[test]
fn plan_emits_recommendations_once_to_stdout_only() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-plan-once")?;
    init_repo(&tmp)?;
    let plan = spawn(&["plan"], &[], &tmp)?;
    assert_eq!(code(&plan), 0);
    let stdout = String::from_utf8_lossy(&plan.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&plan.stderr).into_owned();
    assert!(stdout.contains("Velnor Actions plan"));
    assert!(stderr.is_empty(), "plan stderr must stay empty: {stderr:?}");
    // CLI-5.4: §5's example plus §7 govern over the findings-to-stderr
    // sentence, so the report carries one `Recommendations` section shaped
    // like the example: header on its own line after the workflow section,
    // two-space bodies, stdout only.
    assert_eq!(
        stdout
            .lines()
            .filter(|line| *line == "Recommendations")
            .count(),
        1,
        "one Recommendations header:\n{stdout}"
    );
    let workflow = stdout
        .find("Workflow to generate")
        .ok_or("workflow section")?;
    let header = stdout.find("\nRecommendations\n").ok_or("recs header")?;
    assert!(
        workflow < header,
        "recommendations follow workflow:\n{stdout}"
    );
    let recs = plan_recommendations(&stdout);
    assert!(!recs.is_empty());
    for rec in &recs {
        assert!(
            stdout.lines().any(|line| line == format!("  {rec}")),
            "rec not two-space indented: {rec}"
        );
        let hits = stdout.lines().filter(|line| line.trim() == rec).count()
            + stderr.lines().filter(|line| line.trim() == rec).count();
        assert_eq!(hits, 1, "recommendation emitted {hits}x: {rec}");
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn generate_keeps_recommendations_on_stderr() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-gen-recs")?;
    git_init(&tmp)?;
    assert_eq!(code(&spawn(&["init"], &[], &tmp)?), 0);
    pin_branch(&tmp)?;
    let plan = spawn(&["plan"], &[], &tmp)?;
    assert_eq!(code(&plan), 0);
    let expected = plan_recommendations(&String::from_utf8_lossy(&plan.stdout));
    assert!(!expected.is_empty());
    let outer = fresh_tempdir("smoke-gen-preview")?;
    let preview = outer.join("preview");
    let generated = spawn(
        &["generate", "--output-dir", preview.to_str().unwrap_or("/")],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&generated), 0);
    assert!(generated.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&generated.stderr).into_owned();
    assert!(
        stderr.contains("WARNING: .velnor/release-manifest.json is absent; generated workflows use a debug-only stand-in"),
        "consumer policy must report its debug manifest stand-in: {stderr}"
    );
    for rec in &expected {
        assert!(
            stderr.lines().any(|line| line == rec),
            "generate stderr missing: {rec}"
        );
    }
    assert!(preview.join(".github/workflows/ci.yml").is_file());
    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}

#[test]
fn velnor_generate_omits_consumer_manifest_warning() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-velnor-manifest")?;
    git_init(&tmp)?;
    let config = tmp.join(".velnor").join("config.toml");
    std::fs::create_dir_all(config.parent().ok_or("config parent")?)?;
    std::fs::write(
        &config,
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"main\"\n",
    )?;
    let remote = git_fixture::command(&tmp)?
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ])
        .current_dir(&tmp)
        .output()?;
    assert!(remote.status.success(), "git remote add failed: {remote:?}");
    std::fs::write(
        tmp.join(".velnor").join("release-manifest.json"),
        "not json",
    )?;

    let outer = fresh_tempdir("smoke-velnor-manifest-preview")?;
    let preview = outer.join("preview");
    let generated = spawn(
        &["generate", "--output-dir", preview.to_str().unwrap_or("/")],
        &[],
        &tmp,
    )?;
    let stderr = String::from_utf8_lossy(&generated.stderr).into_owned();
    assert_eq!(code(&generated), 0, "stderr: {stderr}");
    assert!(
        !stderr.contains("WARNING: .velnor/release-manifest.json is absent"),
        "producer policy must not report the consumer-only manifest stand-in: {stderr}"
    );
    assert!(preview.join(".github/workflows/ci.yml").is_file());
    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}

#[test]
fn generate_writes_manual_change_suggestions_for_malformed_tools() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-malformed-tools")?;
    init_repo(&tmp)?;
    // TOOL-2.2: malformed tool files stay read-only inputs; both commands
    // report `tooling_input_invalid` plus a concrete manual-change suggestion.
    std::fs::write(tmp.join("mise.toml"), "[tools\nrust = \n")?;
    std::fs::write(tmp.join("rust-toolchain.toml"), "[[[\n")?;
    let mise_before = std::fs::read(tmp.join("mise.toml"))?;
    let toolchain_before = std::fs::read(tmp.join("rust-toolchain.toml"))?;
    let plan = spawn(&["plan"], &[], &tmp)?;
    assert_eq!(code(&plan), 0, "stderr: {:?}", plan.stderr);
    assert!(plan.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&plan.stdout).into_owned();
    let invalid: Vec<String> = plan_recommendations(&stdout)
        .into_iter()
        .filter(|rec| rec.contains("tooling_input_invalid"))
        .collect();
    assert_eq!(invalid.len(), 2, "both files flagged:\n{stdout}");
    for rec in &invalid {
        assert!(rec.contains("fix it manually"), "no suggestion: {rec}");
    }
    assert!(invalid.iter().any(|rec| rec.contains("mise.toml")));
    assert!(
        invalid
            .iter()
            .any(|rec| rec.contains("rust-toolchain.toml"))
    );
    let outer = fresh_tempdir("smoke-malformed-preview")?;
    let preview = outer.join("preview");
    let generated = spawn(
        &["generate", "--output-dir", preview.to_str().unwrap_or("/")],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&generated), 0, "stderr: {:?}", generated.stderr);
    assert!(generated.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&generated.stderr).into_owned();
    for rec in &invalid {
        assert!(
            stderr.lines().any(|line| line == rec),
            "generate stderr missing: {rec}"
        );
    }
    assert_eq!(std::fs::read(tmp.join("mise.toml"))?, mise_before);
    assert_eq!(
        std::fs::read(tmp.join("rust-toolchain.toml"))?,
        toolchain_before
    );
    assert!(preview.join(".github/workflows/ci.yml").is_file());
    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}

/// Byte-compare two preview trees file by file.
fn assert_same_tree(left: &std::path::Path, right: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let mut pending = vec![left.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let relative = entry.path().strip_prefix(left)?.to_path_buf();
            let other = right.join(&relative);
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                assert_eq!(
                    std::fs::read(entry.path())?,
                    std::fs::read(&other)?,
                    "preview differs at {}",
                    relative.display()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn preview_prints_absolute_paths_and_files() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-abs")?;
    init_repo(&tmp)?;
    add_crate_pair(&tmp)?;
    let outer = fresh_tempdir("smoke-abs-preview")?;
    let preview = outer.join("preview");
    let output = spawn(
        &["generate", "--output-dir", preview.to_str().unwrap_or("/")],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&output), 0, "stderr: {:?}", output.stderr);
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let want_preview = preview.canonicalize().unwrap_or_else(|_| preview.clone());
    let want_root = tmp.canonicalize().unwrap_or_else(|_| tmp.clone());
    assert!(
        stderr.contains(&format!("Preview: {}", want_preview.display())),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("Repository: {}", want_root.display())),
        "{stderr}"
    );
    assert!(stderr.contains(".github/AGENTS.md"), "{stderr}");
    assert!(!stderr.contains("CLAUDE.md"), "{stderr}");
    assert!(stderr.contains(".github/actionlint.yaml"), "{stderr}");
    assert!(stderr.contains(".github/workflows/ci.yml"), "{stderr}");
    assert!(preview.join(".github/AGENTS.md").is_file());
    let claude = preview.join(".github/CLAUDE.md");
    assert!(
        std::fs::symlink_metadata(&claude).is_err(),
        "preview still carries retired .github/CLAUDE.md"
    );
    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}

#[test]
fn previews_use_unique_tmp_dirs_without_collision() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-unique")?;
    init_repo(&tmp)?;
    add_crate_pair(&tmp)?;
    let outer = fresh_tempdir("smoke-unique-previews")?;
    let first = outer.join("first");
    let second = outer.join("second");
    for preview in [&first, &second] {
        let output = spawn(
            &["generate", "--output-dir", preview.to_str().unwrap_or("/")],
            &[],
            &tmp,
        )?;
        assert_eq!(code(&output), 0, "stderr: {:?}", output.stderr);
        assert!(preview.starts_with(std::env::temp_dir()));
        assert!(preview.join(".github/workflows/ci.yml").is_file());
    }
    assert_same_tree(&first.join(".github"), &second.join(".github"))?;
    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}

#[test]
fn public_failures_collapse_to_one_stderr_line() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-single-line")?;
    init_repo(&tmp)?;
    let config = tmp.join(".velnor").join("config.toml");
    let mut body = std::fs::read_to_string(&config)?;
    body.push_str("runner_label = \"x\\ny\"\n");
    std::fs::write(&config, body)?;
    let plan = spawn(&["plan"], &[], &tmp)?;
    assert_eq!(code(&plan), 1);
    let stderr = String::from_utf8_lossy(&plan.stderr).into_owned();
    assert!(stderr.contains("unsupported_label"), "{stderr:?}");
    assert_eq!(stderr.lines().count(), 1, "{stderr:?}");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn help_is_identical_with_and_without_env() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-help")?;
    let plain = spawn(&["--help"], &[], &tmp)?;
    assert_eq!(code(&plain), 0);
    let missing = tmp.join("merge-v1-request.json");
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command
        .arg("--help")
        .current_dir(&tmp)
        .env("VELNOR_INTERNAL_OP", "merge-v1")
        .env("VELNOR_REQUEST_FILE", &missing);
    let gated = command.output()?;
    assert_eq!(code(&gated), 0);
    assert_eq!(gated.stdout, plain.stdout);
    assert_eq!(gated.stderr, plain.stderr);
    cleanup(&tmp);
    Ok(())
}
