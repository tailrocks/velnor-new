// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;

use crate::facts::TestResult;
use crate::helper_integrity::{execute_environment, steps, workflow};
use crate::setup_action::{Install, program};

fn tools(root: &Path, missing: &str) -> TestResult {
    fs::create_dir(root.join("tools"))?;
    for tool in [
        "bash", "mktemp", "cp", "chmod", "mkdir", "env", "shasum", "head", "seq", "sleep", "wc",
        "tr", "rm", "curl", "mv", "cut",
    ] {
        if tool != missing {
            let path = root.join("tools").join(tool);
            if ["mktemp", "cp", "chmod", "env", "shasum"].contains(&tool) {
                fs::write(
                    &path,
                    format!(
                        "#!/bin/bash\nprintf '%s\\n' '{tool}' >> '{}'; exec '{}' \"$@\"\n",
                        root.join("tools-used").display(),
                        program(tool),
                    ),
                )?;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
            } else {
                symlink(program(tool), &path)?;
            }
        }
    }
    Ok(())
}

#[test]
fn setup_records_non_default_tools_and_rendered_calls_use_them() -> TestResult {
    let fixture = Install::new("resolved-helper-tools")?;
    tools(&fixture.work, "")?;
    let yaml = workflow(&"a".repeat(64))?;
    let resolve = steps(&yaml)?
        .iter()
        .find(|step| step["name"] == "Resolve released helper tools")
        .ok_or("no resolver")?;
    let temp = fixture.work.join("temp");
    let tool_path = fixture.work.join("tools");
    let bindings = [("PATH", tool_path.to_str().ok_or("path encoding")?)];
    let installed = execute_environment(resolve, &temp, &tool_path, "", &bindings)?;
    assert!(
        installed.status.success(),
        "{}{}",
        String::from_utf8_lossy(&installed.stdout),
        String::from_utf8_lossy(&installed.stderr),
    );
    assert!(
        fixture
            .spawn(None, |command| {
                command.env("PATH", fixture.work.join("tools"));
            })?
            .wait()?
            .success()
    );
    let recorded = fs::read_to_string(temp.join("helper-tools.env"))?;
    for tool in ["mktemp", "cp", "chmod", "mkdir", "env", "shasum"] {
        assert!(recorded.contains(&fixture.work.join("tools").join(tool).display().to_string()));
    }
    let binary = fixture.work.join("temp/velnor-ci-bin/velnor-ci");
    let digest = Command::new(program("shasum"))
        .args(["-a", "256"])
        .arg(binary)
        .output()?;
    assert!(digest.status.success());
    let digest = String::from_utf8(digest.stdout)?;
    let digest = digest.split_whitespace().next().ok_or("no digest")?;
    let yaml = workflow(digest)?;
    fs::write(fixture.work.join("tools-used"), "")?;
    let temp = fixture.work.join("temp");
    for step in steps(&yaml)? {
        if step["run"].is_string() && step["name"] != "Resolve released helper tools" {
            if step["name"] == "velnor-ci plan" {
                fs::write(fixture.work.join("tools-used"), "")?;
            }
            let output =
                execute_environment(step, &temp, &fixture.work.join("tools"), digest, &bindings)?;
            assert!(
                output.status.success(),
                "{step:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            if step["name"] == "velnor-ci plan" {
                assert_eq!(output.stdout, b"genuine\n");
                break;
            }
        }
    }
    let used = fs::read_to_string(fixture.work.join("tools-used"))?;
    for tool in ["mktemp", "cp", "chmod", "env", "shasum"] {
        assert!(
            used.lines().any(|line| line == tool),
            "{tool} was not used: {used}"
        );
    }
    fs::remove_dir_all(&fixture.work)?;
    Ok(())
}

#[test]
fn setup_names_a_missing_required_tool_before_publishing_the_helper() -> TestResult {
    for missing in ["mktemp", "cp"] {
        let fixture = Install::new(&format!("missing-helper-{missing}"))?;
        tools(&fixture.work, missing)?;
        let yaml = workflow(&"a".repeat(64))?;
        let resolve = steps(&yaml)?
            .iter()
            .find(|step| step["name"] == "Resolve released helper tools")
            .ok_or("no resolver")?;
        let tool_path = fixture.work.join("tools");
        let output = execute_environment(
            resolve,
            &fixture.work.join("temp"),
            &tool_path,
            "",
            &[("PATH", tool_path.to_str().ok_or("path encoding")?)],
        )?;
        assert!(!output.status.success());
        let stdout = String::from_utf8(output.stdout)?;
        assert_eq!(
            stdout.trim(),
            format!("::error title=setup-velnor::required tool '{missing}' was not found on PATH")
        );
        assert!(!fixture.work.join("temp/velnor-ci-bin/velnor-ci").exists());
        assert!(!fixture.work.join("path").exists());
        assert!(
            !fixture
                .work
                .join("temp/velnor-trusted-42-1-format")
                .exists()
        );
        fs::remove_dir_all(&fixture.work)?;
    }
    Ok(())
}
