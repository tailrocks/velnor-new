// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{os::unix::fs::PermissionsExt, path::Path};

fn source(length: usize, value: &str) -> String {
    let prefix = format!("value='é'\nprintf '{value}' > \"$SENTINEL\"\n");
    format!("{}{}", prefix, " ".repeat(length - prefix.len()))
}

fn materialize_step(
    step: &Yaml,
    root: &Path,
    directory: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::new("/bin/bash")
        .args([
            "--noprofile",
            "--norc",
            "-e",
            "-o",
            "pipefail",
            "-c",
            run(step)?,
        ])
        .env("RUNNER_TEMP", root)
        .env("GITHUB_ENV", root.join("env"))
        .env("VELNOR_FIXED_SCRIPT_DIR", directory)
        .output()?)
}

fn directory(root: &Path) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root.join("env"))?
        .lines()
        .find_map(|line| line.strip_prefix("VELNOR_FIXED_SCRIPT_DIR="))
        .ok_or("directory missing")?
        .to_owned())
}

fn run_writers(steps: &[Yaml], root: &Path) -> Result<String, Box<dyn Error>> {
    fs::write(root.join("env"), "")?;
    let mut dir = String::new();
    for step in steps {
        assert!(run(step)?.chars().count() <= 21_000);
        let output = materialize_step(step, root, &dir)?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        dir = directory(root)?;
    }
    Ok(dir)
}

#[test]
fn full_unicode_chunks_round_trip_in_private_files() -> TestResult {
    for length in [44_998, 44_999] {
        for shell in [None, Some("bash")] {
            let source = source(length, "safe");
            let mut document = workflow(&source, "ubuntu-26.04", shell, vec![]);
            bound_workflow(&mut document)?;
            let steps = steps(&document)?;
            let (executor, writers) = steps.split_last().ok_or("executor missing")?;
            let temp = tempfile::tempdir()?;
            let dir = run_writers(writers, temp.path())?;
            assert_eq!(fs::metadata(&dir)?.permissions().mode() & 0o777, 0o700);
            assert_eq!(
                fs::metadata(format!("{dir}/script.sh"))?
                    .permissions()
                    .mode()
                    & 0o777,
                0o400
            );
            assert_eq!(fs::read(format!("{dir}/script.sh"))?, source.as_bytes());
            assert!(!Path::new(&format!("{dir}/script.sh.b64")).exists());
            let sentinel = temp.path().join("sentinel");
            let output = Command::new("/bin/bash")
                .args(["-e", "-c", run(executor)?])
                .env("VELNOR_FIXED_SCRIPT_DIR", &dir)
                .env("SENTINEL", &sentinel)
                .output()?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(sentinel)?, "safe");
        }
    }
    Ok(())
}

#[test]
fn malformed_or_changed_encoded_source_never_executes() -> TestResult {
    for invalid in [true, false] {
        let original = source(44_998, "safe");
        let altered = source(44_998, "evil");
        let mut document = workflow(&original, "ubuntu-26.04", None, vec![]);
        bound_workflow(&mut document)?;
        let steps = steps(&document)?;
        let (executor, writers) = steps.split_last().ok_or("executor missing")?;
        let (last_writer, preceding) = writers.split_last().ok_or("writer missing")?;
        let temp = tempfile::tempdir()?;
        let dir = run_writers(preceding, temp.path())?;
        let encoded = Path::new(&dir).join("script.sh.b64");
        if invalid {
            use std::io::Write;
            fs::OpenOptions::new()
                .append(true)
                .open(&encoded)?
                .write_all(b"!")?;
        } else {
            let replacement = STANDARD.encode(altered.as_bytes());
            let written = usize::try_from(fs::metadata(&encoded)?.len())?;
            fs::write(&encoded, &replacement.as_bytes()[..written])?;
        }
        let last = materialize_step(last_writer, temp.path(), &dir)?;
        if invalid {
            assert!(!last.status.success(), "invalid base64 was accepted");
            assert!(!Path::new(&dir).join("script.sh").exists());
            assert!(!temp.path().join("sentinel").exists());
        } else {
            assert!(
                last.status.success(),
                "{}",
                String::from_utf8_lossy(&last.stderr)
            );
            let sentinel = temp.path().join("sentinel");
            let output = Command::new("/bin/bash")
                .args(["-e", "-c", run(executor)?])
                .env("VELNOR_FIXED_SCRIPT_DIR", &dir)
                .env("SENTINEL", &sentinel)
                .output()?;
            assert!(!output.status.success(), "changed script was executed");
            assert!(!sentinel.exists());
        }
    }
    Ok(())
}
