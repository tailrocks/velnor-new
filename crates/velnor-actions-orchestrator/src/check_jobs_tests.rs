// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
};
use velnor_actions_contract::{ReleaseTarget, StepKind};
use velnor_actions_mise::catalog::lock::ApprovedMiseRuntime;

fn run(case: &str) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let expected = b"approved runtime fixture\n";
    let data = if case == "wrong-hash" {
        b"modified runtime fixture\n".as_slice()
    } else {
        expected.as_slice()
    };
    let input = root.join("download");
    fs::write(&input, data)?;
    let curl = root.join("fake-curl");
    fs::write(
        &curl,
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then shift; /bin/cp \"$FIXTURE_SOURCE\" \"$1\"; exit; fi; shift; done\nexit 1\n",
    )?;
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755))?;
    let runtime = ApprovedMiseRuntime {
        target: ReleaseTarget::LinuxX86_64,
        version: "2026.10.19".to_owned(),
        sha256: crate::generator::sha256_hex(expected),
        bytes: u64::try_from(expected.len())? + u64::from(case == "wrong-size")
            - u64::from(case == "extra-size"),
    };
    let step = super::approved_check_mise_step(&runtime)?;
    let StepKind::Shell { run, .. } = step.kind else {
        return Err("expected shell".into());
    };
    let script = run
        .get(2)
        .ok_or("script")?
        .replace("/usr/bin/curl", curl.to_str().ok_or("curl path")?);
    let victim = root.join("victim");
    fs::create_dir(&victim)?;
    fs::write(victim.join("sentinel"), b"unchanged")?;
    match case {
        "parent-symlink" => symlink(&victim, root.join("velnor"))?,
        "target-symlink" => {
            fs::create_dir(root.join("velnor"))?;
            symlink(&victim, root.join("velnor/check-mise"))?;
        }
        _ => {}
    }
    let output = Command::new("bash")
        .args(["-c", &script])
        .env("RUNNER_TEMP", root)
        .env("FIXTURE_SOURCE", &input)
        .output()?;
    assert_eq!(
        output.status.success(),
        case == "valid",
        "{case}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let destination = root.join("velnor/check-mise/bin/mise");
    if case == "valid" {
        assert_eq!(fs::read(&destination)?, expected);
        for directory in ["velnor", "velnor/check-mise", "velnor/check-mise/bin"] {
            assert_eq!(
                fs::metadata(root.join(directory))?.permissions().mode() & 0o777,
                0o700
            );
        }
        assert_eq!(
            fs::metadata(&destination)?.permissions().mode() & 0o777,
            0o500
        );
    } else if matches!(case, "parent-symlink" | "target-symlink") {
        assert!(!victim.join("check-mise").exists());
        assert!(!victim.join("bin").exists());
    } else if destination.exists() {
        assert_ne!(
            fs::metadata(&destination)?.permissions().mode() & 0o777,
            0o500
        );
    }
    assert_eq!(fs::read(victim.join("sentinel"))?, b"unchanged");
    Ok(())
}

#[test]
fn approved_check_mise_accepts_only_exact_bytes_in_private_directory() -> Result<(), Box<dyn Error>>
{
    for case in [
        "valid",
        "wrong-hash",
        "wrong-size",
        "extra-size",
        "parent-symlink",
        "target-symlink",
    ] {
        run(case)?;
    }
    Ok(())
}
