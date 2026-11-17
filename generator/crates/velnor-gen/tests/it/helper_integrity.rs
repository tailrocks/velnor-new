// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use velnor_model::{
    CargoCommand, CommitSha, CratesMirror, HelperCommand, HelperRelease, HelperVersion, Platform,
    RunnerLabel, Sha256, StepKind, VerdictSpec, WORKFLOW_PATH, When,
};
use velnor_render::render;

use crate::facts::TestResult;
use crate::io::Scratch;

const GOOD: &str = "#!/bin/sh\nprintf 'released-helper\\n'\n";
const BAD: &str = "#!/bin/sh\nprintf 'replaced-helper\\n'\n";

fn workflow(digest: &str) -> Result<Value, Box<dyn Error>> {
    let release = HelperRelease::new(
        HelperVersion::new("1.2.3")?,
        CommitSha::new("1793cf84b5924e2542a03b0fb2580e7395b2f4cb")?,
        Platform::ALL
            .into_iter()
            .enumerate()
            .map(|(index, platform)| {
                Ok((
                    platform,
                    Sha256::new(if index == 0 {
                        digest.to_owned()
                    } else {
                        index.to_string().repeat(64)
                    })?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?,
    )?;
    let mut plan = crate::rules::plan(vec![
        StepKind::InstallHelper(release),
        StepKind::Helper(HelperCommand::Plan),
        StepKind::Cargo(CargoCommand::BuildAll),
        StepKind::CratesMirror(CratesMirror {
            runner: RunnerLabel::new("mirror-box")?,
            registry: "sparse+https://mirror.example/index/".to_owned(),
            cainfo: "/opt/mirror.pem".to_owned(),
        }),
        StepKind::Verdict {
            spec: VerdictSpec::parse("plan", &["plan=always".to_owned()], &[])?,
            needs_max_bytes: 100,
        },
    ])?;
    plan.jobs[0].steps.last_mut().ok_or("no verdict")?.when = When::Always;
    let files = render(&plan).map_err(|errors| format!("{errors:?}"))?;
    Ok(serde_saphyr::from_str(
        files.get(&WORKFLOW_PATH).ok_or("no workflow")?,
    )?)
}

fn steps(workflow: &Value) -> Result<&Vec<Value>, Box<dyn Error>> {
    workflow["jobs"]["format"]["steps"]
        .as_array()
        .ok_or_else(|| "no steps".into())
}

#[test]
fn every_trusted_helper_use_copies_verifies_and_executes_one_fresh_path() -> TestResult {
    let workflow = workflow(&"a".repeat(64))?;
    let steps = steps(&workflow)?;
    let mut seen = Vec::new();
    for step in steps {
        let Some(run) = step["run"].as_str() else {
            continue;
        };
        if !run.starts_with("velnor-ci ") && step["env"]["VELNOR_HELPER"].is_null()
            || step["env"]["VELNOR_HELPER_SOURCE"].is_string()
        {
            continue;
        }
        assert_eq!(run.lines().count(), 1, "{run}");
        let mut rest = run;
        for operation in [
            "VELNOR_HELPER_DIRECTORY=$(/usr/bin/mktemp -d \"$VELNOR_HELPER_TEMP/velnor-helper.XXXXXXXXXX\")",
            "VELNOR_HELPER_COPY=$(/usr/bin/mktemp \"$VELNOR_HELPER_DIRECTORY/velnor-ci.XXXXXXXXXX\")",
            "/bin/cp \"$VELNOR_HELPER\" \"$VELNOR_HELPER_COPY\"",
            "/bin/chmod 500 \"$VELNOR_HELPER_COPY\"",
            "\"$VELNOR_HELPER_CHECKSUM\" $VELNOR_HELPER_CHECKSUM_ARGUMENTS <<< \"$VELNOR_HELPER_SHA256  $VELNOR_HELPER_COPY\"",
            "exec \"$VELNOR_HELPER_COPY\" ",
        ] {
            rest = rest
                .strip_prefix(operation)
                .ok_or_else(|| format!("missing {operation}: {run}"))?;
            if !operation.starts_with("exec ") {
                rest = rest
                    .strip_prefix(" && ")
                    .ok_or("operation not conditional")?;
            }
        }
        assert_eq!(step["env"]["VELNOR_HELPER_TEMP"], "${{ runner.temp }}");
        assert!(
            !step["if"]
                .as_str()
                .unwrap_or_default()
                .contains("velnor-helper-check")
        );
        let digest = step["env"]["VELNOR_HELPER_SHA256"]
            .as_str()
            .ok_or("no digest")?;
        for (os, arch, pin) in [
            ("Linux", "X64", "a".repeat(64)),
            ("Linux", "ARM64", "1".repeat(64)),
            ("macOS", "ARM64", "2".repeat(64)),
        ] {
            assert!(
                digest.contains(&format!(
                    "runner.os == '{os}' && runner.arch == '{arch}' && '{pin}'"
                )),
                "{digest}"
            );
        }
        assert!(digest.ends_with(" || '' }}"), "{digest}");
        seen.push(rest);
    }
    for command in ["plan ", "run --", "crates-mirror ", "gate "] {
        assert!(
            seen.iter().any(|run| run.starts_with(command)),
            "missing {command}: {seen:?}"
        );
    }
    let gate = steps
        .iter()
        .find(|step| step["name"] == "velnor-ci gate")
        .ok_or("no gate")?;
    assert_eq!(gate["if"], "${{ always() }}");
    assert!(
        !steps
            .iter()
            .any(|step| step["name"] == "Verify the released helper")
    );
    Ok(())
}

fn execute(
    step: &Value,
    root: &Path,
    shadows: &Path,
    digest: &str,
) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new("/bin/bash");
    command.args([
        "--noprofile",
        "--norc",
        "-e",
        "-o",
        "pipefail",
        "-c",
        step["run"].as_str().ok_or("no run")?,
    ]);
    command
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", shadows.display()));
    if let Some(env) = step["env"].as_object() {
        for (key, value) in env {
            let text = value.as_str().ok_or("non-string environment")?;
            let text = if key == "VELNOR_HELPER_SHA256" {
                digest.to_owned()
            } else if key == "VELNOR_HELPER_CHECKSUM" {
                assert!(text.contains("/usr/bin/sha256sum") && text.contains("/usr/bin/shasum"));
                if cfg!(target_os = "macos") {
                    "/usr/bin/shasum"
                } else {
                    "/usr/bin/sha256sum"
                }
                .to_owned()
            } else if key == "VELNOR_HELPER_CHECKSUM_ARGUMENTS" {
                if cfg!(target_os = "macos") {
                    "-a 256 --check --status"
                } else {
                    "--check --status"
                }
                .to_owned()
            } else {
                text.replace("${{ runner.temp }}", root.to_str().ok_or("non-UTF8 root")?)
                    .replace("${{ github.run_id }}", "42")
                    .replace("${{ github.run_attempt }}", "1")
                    .replace("${{ github.job }}", "format")
            };
            command.env(key, text);
        }
    }
    Ok(command.output()?)
}

fn fixture() -> Result<(Scratch, String), Box<dyn Error>> {
    let scratch = Scratch::new("helper-integrity")?;
    let root = scratch.outside();
    let source = root.join("velnor-ci-bin/velnor-ci");
    fs::create_dir_all(source.parent().ok_or("no source parent")?)?;
    fs::write(&source, GOOD)?;
    fs::set_permissions(&source, fs::Permissions::from_mode(0o700))?;
    let hash = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(&source)
        .output()?;
    assert!(hash.status.success());
    let digest = String::from_utf8(hash.stdout)?
        .split_whitespace()
        .next()
        .ok_or("no digest")?
        .to_owned();
    let shadows = root.join("shadows");
    fs::create_dir(&shadows)?;
    for program in ["velnor-ci", "sha256sum", "shasum"] {
        let file = shadows.join(program);
        fs::write(&file, BAD)?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o700))?;
    }
    Ok((scratch, digest))
}

#[test]
fn rendered_commands_ignore_path_shadows_and_refuse_a_replaced_helper() -> TestResult {
    let (scratch, digest) = fixture()?;
    let root = scratch.outside();
    let shadows = root.join("shadows");
    let workflow = workflow(&digest)?;
    let steps = steps(&workflow)?;
    let use_index = steps
        .iter()
        .position(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.ends_with(" plan --explain"))
        })
        .ok_or("no plan use")?;
    for step in &steps[..use_index] {
        if step["run"].is_string() {
            let result = execute(step, &root, &shadows, &digest)?;
            assert!(
                result.status.success(),
                "{step:?}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    let genuine = execute(&steps[use_index], &root, &shadows, &digest)?;
    assert!(genuine.status.success());
    assert_eq!(genuine.stdout, b"released-helper\n");
    let path = steps[use_index]["env"]["VELNOR_HELPER"]
        .as_str()
        .ok_or("no installed path")?
        .replace("${{ runner.temp }}", root.to_str().ok_or("root encoding")?)
        .replace("${{ github.run_id }}", "42")
        .replace("${{ github.run_attempt }}", "1")
        .replace("${{ github.job }}", "format");
    let installed = Path::new(&path);
    assert!(!installed.starts_with(scratch.repo()));
    assert_eq!(fs::metadata(installed)?.permissions().mode() & 0o777, 0o500);
    assert_eq!(
        fs::metadata(installed.parent().ok_or("no installed parent")?)?
            .permissions()
            .mode()
            & 0o777,
        0o500
    );
    for invalid in ["", &"0".repeat(64)] {
        assert!(
            !execute(&steps[use_index], &root, &shadows, invalid)?
                .status
                .success()
        );
    }
    fs::set_permissions(installed, fs::Permissions::from_mode(0o700))?;
    fs::write(installed, BAD)?;
    let rejected = execute(&steps[use_index], &root, &shadows, &digest)?;
    assert!(
        !rejected.status.success(),
        "replaced helper executed: {}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    assert!(!String::from_utf8_lossy(&rejected.stdout).contains("replaced-helper"));
    fs::set_permissions(
        installed.parent().ok_or("no installed parent")?,
        fs::Permissions::from_mode(0o700),
    )?;
    Ok(())
}

#[test]
fn trusted_helper_arguments_still_reject_shell_syntax() -> TestResult {
    use velnor_render::{RenderedJob, RenderedStep, RenderedWorkflow, Rule, check_rules};

    let yaml = workflow(&"a".repeat(64))?;
    let command = steps(&yaml)?
        .iter()
        .find(|step| step["name"] == "velnor-ci plan")
        .and_then(|step| step["run"].as_str())
        .ok_or("no helper command")?;
    for (suffix, symbol) in [("; BAD", ";"), (" $(BAD)", "$(")] {
        let result = check_rules(&RenderedWorkflow {
            jobs: vec![RenderedJob {
                id: "format".to_owned(),
                runs_on: "ubuntu-latest".to_owned(),
                timeout_minutes: 10,
                steps: vec![RenderedStep {
                    name: "trusted helper".to_owned(),
                    run: Some(format!("{command}{suffix}")),
                    ..RenderedStep::default()
                }],
                ..RenderedJob::default()
            }],
        });
        assert!(result.is_err_and(|violations| {
            violations
                .iter()
                .any(|violation| violation.rule == Rule::ForbiddenSyntax(symbol))
        }));
    }
    Ok(())
}
