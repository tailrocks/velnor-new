// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::helper_integrity::{execute_environment, fixture, steps, workflow};
use std::fs;
use std::os::unix::fs::PermissionsExt;

const SHELL: &str = "bash -p {0}";

pub(crate) fn native_helper(root: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let source = root.join("velnor-ci-bin/velnor-ci");
    fs::copy("/bin/echo", &source)?;
    if cfg!(target_os = "macos") {
        let signed = std::process::Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&source)
            .output()?;
        assert!(
            signed.status.success(),
            "{}",
            String::from_utf8_lossy(&signed.stderr)
        );
    }

    let hash = std::process::Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(&source)
        .output()?;
    assert!(hash.status.success());
    let digest = String::from_utf8(hash.stdout)?
        .split_whitespace()
        .next()
        .ok_or("no digest")?
        .to_owned();
    Ok(digest)
}

fn poisoned_invocation(poison: &str) -> TestResult {
    let (scratch, _) = fixture()?;
    let root = scratch.outside();
    let digest = native_helper(&root)?;
    let shadows = root.join("shadows");
    for name in ["bash", "sh"] {
        let shadow = shadows.join(name);
        fs::write(&shadow, "#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(shadow, fs::Permissions::from_mode(0o700))?;
    }

    let yaml = workflow(&digest)?;
    let steps = steps(&yaml)?;
    let index = steps
        .iter()
        .position(|step| step["name"] == "velnor-ci plan")
        .ok_or("no plan")?;
    for step in &steps[..index] {
        if step["run"].is_string() {
            assert!(
                execute_environment(step, &root, &shadows, &digest, &[])?
                    .status
                    .success()
            );
        }
    }
    let startup = root.join("startup.sh");
    fs::write(&startup, "exit 0\n")?;
    let path = startup.to_str().ok_or("startup encoding")?;
    let environment = if poison == "startup" {
        vec![("BASH_ENV", path)]
    } else if poison == "options" {
        vec![("SHELLOPTS", "noexec")]
    } else {
        vec![
            ("BASH_ENV", "/dev/null"),
            ("BASH_FUNC_exec%%", "() { printf 'forged-helper\\n'; }"),
        ]
    };
    let good = execute_environment(&steps[index], &root, &shadows, &digest, &environment)?;
    assert!(
        good.status.success(),
        "{poison}: {:?}: {}",
        good.status,
        String::from_utf8_lossy(&good.stderr)
    );
    assert_eq!(good.stdout, b"plan --explain\n", "{poison}");
    let invalid = execute_environment(
        &steps[index],
        &root,
        &shadows,
        &"0".repeat(64),
        &environment,
    )?;
    assert!(!invalid.status.success(), "{poison}: invalid digest passed");
    let installed = root.join("velnor-trusted-42-1-format/velnor-ci");
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o700))?;
    fs::write(&installed, "#!/bin/sh\nprintf 'replaced-helper\\n'\n")?;
    let replaced = execute_environment(&steps[index], &root, &shadows, &digest, &environment)?;
    assert!(
        !replaced.status.success(),
        "{poison}: replaced helper passed"
    );
    assert!(!String::from_utf8_lossy(&replaced.stdout).contains("replaced-helper"));
    fs::set_permissions(
        installed.parent().ok_or("no parent")?,
        fs::Permissions::from_mode(0o700),
    )?;
    Ok(())
}

#[test]
fn trusted_invocations_ignore_bash_env_startup() -> TestResult {
    poisoned_invocation("startup")
}

#[test]
fn trusted_invocations_ignore_imported_exec_functions() -> TestResult {
    poisoned_invocation("function")
}

#[test]
fn trusted_invocations_ignore_imported_shell_options() -> TestResult {
    poisoned_invocation("options")
}

#[test]

fn hardened_shell_is_allowed_only_with_the_exact_trusted_wrapper() -> TestResult {
    use velnor_render::{RenderedJob, RenderedStep, RenderedWorkflow, Rule, check_rules};
    let yaml = workflow(&"a".repeat(64))?;
    let invocation = steps(&yaml)?
        .iter()
        .find(|step| step["name"] == "velnor-ci plan")
        .and_then(|step| step["run"].as_str())
        .ok_or("no invocation")?;
    let mutated = invocation.replace("$VELNOR_HELPER_EXEC", "$VELNOR_HELPER_COPY");
    for (run, shell, trusted) in [
        (invocation, SHELL, true),
        ("echo ordinary", SHELL, false),
        (mutated.as_str(), SHELL, false),
        (
            invocation,
            "/bin/bash --noprofile --norc -e -o pipefail {0}",
            false,
        ),
    ] {
        let result = check_rules(&RenderedWorkflow {
            jobs: vec![RenderedJob {
                id: "trusted".to_owned(),
                runs_on: "ubuntu-latest".to_owned(),
                timeout_minutes: 10,
                steps: vec![RenderedStep {
                    name: "trusted".to_owned(),
                    run: Some(run.to_owned()),
                    shell: Some(shell.to_owned()),
                    ..RenderedStep::default()
                }],
                ..RenderedJob::default()
            }],
        });
        let refused = result.is_err_and(|violations| {
            violations
                .iter()
                .any(|violation| violation.rule == Rule::ShellKey)
        });
        assert_eq!(refused, !trusted, "{run} {shell}");
    }
    Ok(())
}
