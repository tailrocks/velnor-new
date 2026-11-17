// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::helper_integrity::{execute_environment, fixture, steps, workflow};
use crate::helper_startup::native_helper;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn trusted_shasum_ignores_perl_startup_and_inherited_launcher_poison() -> TestResult {
    let (scratch, _) = fixture()?;
    let root = scratch.outside();
    let digest = native_helper(&root)?;
    let shadows = root.join("shadows");
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
    fs::write(
        root.join("HelperPoison.pm"),
        "package HelperPoison; BEGIN { exit 0; } 1;\n",
    )?;
    let poison = [
        ("PERL5OPT", "-MHelperPoison"),
        ("PERL5LIB", root.to_str().ok_or("non UTF8 root")?),
        ("VELNOR_HELPER_LAUNCHER", "printf 'forged-helper\\n'"),
    ];
    let mut step = steps[index].clone();
    step["env"]["VELNOR_HELPER_CHECKSUM"] = "/usr/bin/shasum".into();
    step["env"]["VELNOR_HELPER_CHECKSUM_ARGUMENTS"] = "-a 256 --check --status".into();
    let good = execute_environment(&step, &root, &shadows, &digest, &poison)?;
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    assert_eq!(good.stdout, b"plan --explain\n");
    for invalid in ["", &"0".repeat(64)] {
        let rejected = execute_environment(&step, &root, &shadows, invalid, &poison)?;
        assert!(
            !rejected.status.success(),
            "poisoned shasum accepted invalid digest"
        );
        assert_eq!(rejected.stdout, b"");
        assert!(
            rejected
                .stderr
                .ends_with(b"velnor-ci integrity check failed\n")
        );
    }
    let installed = root.join("velnor-trusted-42-1-format/velnor-ci");
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o700))?;
    fs::write(&installed, "#!/bin/sh\nprintf 'replaced-helper\\n'\n")?;
    let rejected = execute_environment(&step, &root, &shadows, &digest, &poison)?;
    assert!(!rejected.status.success());
    assert_eq!(rejected.stdout, b"");
    assert!(
        rejected
            .stderr
            .ends_with(b"velnor-ci integrity check failed\n")
    );
    fs::remove_dir_all(installed.parent().ok_or("no parent")?)?;
    Ok(())
}

#[test]
fn trusted_launcher_preserves_helper_arguments_and_failure_status() -> TestResult {
    let (scratch, _) = fixture()?;
    let root = scratch.outside();
    let source = root.join("velnor-ci-bin/velnor-ci");
    fs::write(&source, "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\nexit 7\n")?;
    let hashed = std::process::Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(&source)
        .output()?;
    assert!(hashed.status.success());
    let digest = String::from_utf8(hashed.stdout)?
        .split_whitespace()
        .next()
        .ok_or("no digest")?
        .to_owned();
    let yaml = workflow(&digest)?;
    let steps = steps(&yaml)?;
    let index = steps
        .iter()
        .position(|step| step["name"] == "velnor-ci plan")
        .ok_or("no plan")?;
    let shadows = root.join("shadows");
    for step in &steps[..index] {
        if step["run"].is_string() {
            assert!(
                execute_environment(step, &root, &shadows, &digest, &[])?
                    .status
                    .success()
            );
        }
    }
    let mut invocation = steps[index].clone();
    let run = invocation["run"]
        .as_str()
        .ok_or("no run")?
        .replace("plan --explain", "plan 'two words' ''");
    invocation["run"] = run.into();
    let result = execute_environment(&invocation, &root, &shadows, &digest, &[])?;
    assert_eq!(result.status.code(), Some(7));
    assert_eq!(result.stdout, b"<plan>\n<two words>\n<>\n");
    fs::remove_dir_all(root.join("velnor-trusted-42-1-format"))?;
    Ok(())
}
