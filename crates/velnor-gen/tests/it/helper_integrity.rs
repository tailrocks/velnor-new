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

pub(crate) const DEFAULT_SHELL: &str = "bash -p {0}";
const GOOD: &str = "#!/bin/sh\nprintf 'released-helper\\n'\n";
const BAD: &str = "#!/bin/sh\nprintf 'replaced-helper\\n'\n";

pub(crate) fn workflow_text(digest: &str, extra: Vec<StepKind>) -> Result<String, Box<dyn Error>> {
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
    let template = plan.jobs[0].steps[1].clone();
    plan.jobs[0]
        .steps
        .extend(extra.into_iter().map(|kind| velnor_model::Step {
            kind,
            ..template.clone()
        }));
    let files = render(&plan).map_err(|errors| format!("{errors:?}"))?;
    Ok(files.get(&WORKFLOW_PATH).ok_or("no workflow")?.clone())
}

pub(crate) fn workflow(digest: &str) -> Result<Value, Box<dyn Error>> {
    Ok(crate::yaml_tree::document(&workflow_text(
        digest,
        Vec::new(),
    )?)?)
}

pub(crate) fn steps(workflow: &Value) -> Result<&Vec<Value>, Box<dyn Error>> {
    workflow["jobs"]["format"]["steps"]
        .as_array()
        .ok_or_else(|| "no steps".into())
}

#[test]
fn every_trusted_helper_use_runs_the_one_installed_launcher() -> TestResult {
    let workflow = workflow(&"a".repeat(64))?;
    assert_eq!(workflow["defaults"]["run"]["shell"], DEFAULT_SHELL);
    let steps = steps(&workflow)?;
    let mut seen = Vec::new();
    for step in steps {
        let Some(run) = step["run"].as_str() else {
            continue;
        };
        let Some(rest) = run.strip_prefix(". \"$VELNOR_EXEC\" ") else {
            assert!(!run.contains("$VELNOR_EXEC"), "{run}");
            continue;
        };
        assert!(step["shell"].is_null());
        assert_eq!(run.lines().count(), 1, "{run}");
        assert!(
            step["env"]
                .as_object()
                .is_none_or(|env| env.keys().all(|key| !key.starts_with("VELNOR_HELPER"))),
            "{step:?}"
        );
        assert!(
            !step["if"]
                .as_str()
                .unwrap_or_default()
                .contains("velnor-helper-check")
        );
        seen.push(rest);
    }
    for command in ["plan ", "run --", "crates-mirror ", "gate "] {
        assert!(
            seen.iter().any(|run| run.starts_with(command)),
            "missing {command}: {seen:?}"
        );
    }
    let installs: Vec<_> = steps
        .iter()
        .filter(|step| step["name"] == "Install the released helper launcher")
        .collect();
    assert_eq!(installs.len(), 1);
    let launcher = installs[0]["env"]["VELNOR_HELPER_LAUNCHER"]
        .as_str()
        .ok_or("no launcher")?;
    assert!(launcher.contains("\"$VELNOR_HELPER_ENV\" -i PATH=/usr/bin:/bin"));
    assert!(launcher.ends_with("exec \"$copy\" \"$@\""));
    let digest = installs[0]["env"]["VELNOR_HELPER_SHA256"]
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
    execute_environment(step, root, shadows, digest, &[])
}

pub(crate) fn execute_environment(
    step: &Value,
    root: &Path,
    shadows: &Path,
    digest: &str,
    environment: &[(&str, &str)],
) -> Result<Output, Box<dyn Error>> {
    let script = root.join("invocation.sh");
    fs::write(&script, step["run"].as_str().ok_or("no run")?)?;
    let env_file = root.join("helper-tools.env");
    let mut bindings: BTreeMap<String, String> = fs::read_to_string(&env_file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    if step["run"]
        .as_str()
        .is_some_and(|run| run.contains("$VELNOR_EXEC"))
    {
        bindings.insert("VELNOR_HELPER_SHA256".to_owned(), digest.to_owned());
    }
    bindings.extend(
        environment
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
    let shell = step["shell"].as_str().unwrap_or(DEFAULT_SHELL);
    let mut words = shell.split_whitespace();
    let program = words.next().ok_or("no shell executable")?;
    let mut command = Command::new(if program == "bash" {
        "/bin/bash"
    } else {
        program
    });
    for word in words {
        command.arg(if word == "{0}" {
            script.as_os_str()
        } else {
            word.as_ref()
        });
    }
    let path = if step["env"]["VELNOR_HELPER_INSTALL_TOOLS"].is_string() {
        "/usr/bin:/bin".to_owned()
    } else {
        format!("{}:/usr/bin:/bin", shadows.display())
    };
    command
        .env_clear()
        .env("PATH", path)
        .env("GITHUB_ENV", env_file)
        .env("RUNNER_TEMP", root)
        .env("GITHUB_RUN_ID", "42")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_JOB", "format")
        .envs(&bindings);
    if let Some(env) = step["env"].as_object() {
        for (key, value) in env {
            let text = value.as_str().ok_or("non-string environment")?;
            let text = if key == "VELNOR_HELPER_SHA256" {
                digest.to_owned()
            } else if let Some(name) = text
                .strip_prefix("${{ env.")
                .and_then(|text| text.strip_suffix(" }}"))
            {
                bindings
                    .get(name)
                    .ok_or("missing installed tool binding")?
                    .clone()
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

pub(crate) fn fixture() -> Result<(Scratch, String), Box<dyn Error>> {
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
    let installed = &root.join("velnor-trusted-42-1-format/velnor-ci");
    assert!(!installed.starts_with(scratch.repo()));
    assert_eq!(fs::metadata(installed)?.permissions().mode() & 0o777, 0o500);
    assert_eq!(
        fs::metadata(installed.parent().ok_or("no installed parent")?)?
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for invalid in ["", &"0".repeat(64)] {
        let rejected = execute(&steps[use_index], &root, &shadows, invalid)?;
        assert!(!rejected.status.success());
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("velnor-ci integrity check failed"),
            "{}",
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert!(rejected.stderr.ends_with(b"\n"));
        assert_eq!(rejected.stdout, b"");
    }
    fs::set_permissions(installed, fs::Permissions::from_mode(0o700))?;
    fs::write(installed, BAD)?;
    let rejected = execute(&steps[use_index], &root, &shadows, &digest)?;
    assert!(
        !rejected.status.success(),
        "replaced helper executed: {}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("velnor-ci integrity check failed"),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(rejected.stderr.ends_with(b"\n"));
    assert_eq!(rejected.stdout, b"");
    fs::set_permissions(installed, fs::Permissions::from_mode(0o500))?;
    fs::remove_dir_all(installed.parent().ok_or("no installed parent")?)?;
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

#[test]
fn only_the_steps_that_run_the_launcher_are_marked_for_the_shellcheck_it_cannot_follow()
-> TestResult {
    const MARK: &str = "# jactionlint ignore=shellcheck";
    let text = workflow_text(&"a".repeat(64), Vec::new())?;
    let lines: Vec<_> = text.lines().collect();
    let mut launcher_steps = 0;
    let mut step_at = None;
    for (at, line) in lines.iter().enumerate() {
        if line.starts_with("      - ") {
            step_at = Some(at);
        }
        if !line.trim_start().starts_with("run: '. \"$VELNOR_EXEC\" ") {
            continue;
        }
        let step = step_at.ok_or("invocation outside a step")?;
        assert_eq!(lines[step - 1].trim(), MARK, "{line}");
        launcher_steps += 1;
    }
    assert!(launcher_steps > 0, "{text}");
    let marks = lines.iter().filter(|line| line.trim() == MARK).count();
    assert_eq!(
        marks, launcher_steps,
        "a mark outside a launcher step: {text}"
    );
    for (at, line) in lines.iter().enumerate() {
        let job = line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':');
        assert!(!job || lines[at - 1].trim() != MARK, "{line}");
    }
    Ok(())
}
