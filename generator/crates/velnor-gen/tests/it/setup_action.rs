// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_render::SETUP_VELNOR_ACTION;

type TestResult = Result<(), Box<dyn Error>>;

const REPOSITORY: &str = "tailrocks/velnor-new/";
const INSTALL_STEP: &str = "      - name: Install velnor-ci";

fn indented_keys(text: &str, parent: &str, indent: usize) -> BTreeSet<String> {
    let pad = " ".repeat(indent);
    let header = format!("{pad}{parent}:");
    text.lines()
        .skip_while(|line| *line != header)
        .skip(1)
        .take_while(|line| line.starts_with(&format!("{pad} ")))
        .filter_map(|line| {
            let rest = line.strip_prefix(&format!("{pad}  "))?;
            let (key, _) = rest.split_once(':')?;
            (!key.starts_with(' ')).then(|| key.to_owned())
        })
        .collect()
}

#[test]
fn the_action_declares_exactly_the_inputs_the_generated_install_step_passes() -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let action_dir = SETUP_VELNOR_ACTION
        .strip_prefix(REPOSITORY)
        .ok_or("the setup action is not in this repository")?;
    let action = fs::read_to_string(root.join(action_dir).join("action.yml"))?;
    assert!(action.contains("using: composite"));

    let workflow = fs::read_to_string(
        root.join("generator/samples/helper-lock/expected/.github/workflows/rust-ci.yml"),
    )?;
    let step: String = workflow
        .lines()
        .skip_while(|line| !line.starts_with(INSTALL_STEP))
        .enumerate()
        .take_while(|(index, line)| *index == 0 || line.starts_with("        "))
        .flat_map(|(_, line)| [line, "\n"])
        .collect();
    assert!(
        step.contains(&format!("uses: {SETUP_VELNOR_ACTION}@")),
        "{step}"
    );

    let declared = indented_keys(&action, "inputs", 0);
    let passed = indented_keys(&step, "with", 8);
    assert_eq!(passed.len(), 4, "{passed:?}");
    assert_eq!(declared, passed);
    Ok(())
}

fn install_script(action: &str) -> String {
    action
        .lines()
        .skip_while(|line| *line != "      run: |")
        .skip(1)
        .map(|line| line.strip_prefix("        ").unwrap_or(line))
        .flat_map(|line| [line, "\n"])
        .collect()
}

#[test]
fn the_binary_on_path_is_a_private_copy_that_a_change_to_the_tool_cache_cannot_reach() -> TestResult
{
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let action = fs::read_to_string(root.join("actions/setup-velnor/action.yml"))?;
    let script_text = install_script(&action);
    assert!(
        !script_text.contains("mktemp -d \"$versions"),
        "verified bytes are staged inside the shared tool cache"
    );
    let work = std::env::temp_dir().join(format!("velnor-gen-setup-action-{}", std::process::id()));
    fs::create_dir_all(&work)?;
    let releases = work.join("releases");
    let release = releases.join("rust-ci-v1.2.3");
    fs::create_dir_all(&release)?;
    let binary = b"#!/bin/sh\necho genuine\n";
    fs::write(release.join("velnor-ci-linux-x64"), binary)?;
    let digest = Command::new("shasum")
        .args(["-a", "256"])
        .arg(release.join("velnor-ci-linux-x64"))
        .output()?
        .stdout;
    let digest = String::from_utf8(digest)?;
    let digest = digest.split_whitespace().next().ok_or("no digest")?;
    let (cache, temp) = (work.join("cache"), work.join("temp"));
    fs::create_dir_all(&temp)?;
    let path_file = work.join("path");
    let script = work.join("install.sh");
    fs::write(&script, script_text)?;
    let status = Command::new("bash")
        .arg(&script)
        .env("VERSION", "1.2.3")
        .env("SHA256_LINUX_X64", digest)
        .env("SHA256_LINUX_ARM64", "")
        .env("SHA256_MACOS_ARM64", "")
        .env("RELEASES", format!("file://{}", releases.display()))
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("RUNNER_TOOL_CACHE", &cache)
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_PATH", &path_file)
        .status()?;
    assert!(status.success());
    let on_path = fs::read_to_string(&path_file)?;
    let installed = Path::new(on_path.trim()).join("velnor-ci");
    assert!(!fs::symlink_metadata(&installed)?.file_type().is_symlink());
    assert!(fs::metadata(&installed)?.permissions().mode() & 0o111 != 0);
    let cached = cache.join("velnor-ci/1.2.3/x64/velnor-ci");
    fs::write(&cached, b"#!/bin/sh\necho tampered\n")?;
    let copy = fs::read(&installed)?;
    fs::remove_dir_all(&work)?;
    assert_eq!(copy, binary);
    Ok(())
}

#[test]
fn no_run_block_of_the_action_expands_an_expression() -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let action = fs::read_to_string(root.join("actions/setup-velnor/action.yml"))?;
    let script = install_script(&action);
    assert!(!script.is_empty());
    assert!(
        !script.contains("${{"),
        "inputs must reach the script through env"
    );
    Ok(())
}

struct Install {
    work: std::path::PathBuf,
    digest: String,
}

fn staged_install(tag: &str) -> Result<Install, Box<dyn Error>> {
    use std::process::Command;

    let work = std::env::temp_dir().join(format!("velnor-gen-setup-{tag}-{}", std::process::id()));
    let release = work.join("releases/rust-ci-v1.2.3");
    fs::create_dir_all(&release)?;
    let file = release.join("velnor-ci-linux-x64");
    fs::write(&file, b"#!/bin/sh\necho genuine\n")?;
    let digest = Command::new("shasum")
        .args(["-a", "256"])
        .arg(&file)
        .output()?
        .stdout;
    let digest = String::from_utf8(digest)?
        .split_whitespace()
        .next()
        .ok_or("no digest")?
        .to_owned();
    fs::create_dir_all(work.join("temp"))?;
    Ok(Install { work, digest })
}

fn run_install(install: &Install, cache: &Path) -> Result<bool, Box<dyn Error>> {
    use std::process::Command;
    use std::time::{Duration, Instant};

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let action = fs::read_to_string(root.join("actions/setup-velnor/action.yml"))?;
    let script = install.work.join("install.sh");
    fs::write(&script, install_script(&action))?;
    let mut child = Command::new("bash")
        .arg(&script)
        .env("VERSION", "1.2.3")
        .env("SHA256_LINUX_X64", &install.digest)
        .env("SHA256_LINUX_ARM64", "")
        .env("SHA256_MACOS_ARM64", "")
        .env(
            "RELEASES",
            format!("file://{}", install.work.join("releases").display()),
        )
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("RUNNER_TOOL_CACHE", cache)
        .env("RUNNER_TEMP", install.work.join("temp"))
        .env("GITHUB_PATH", install.work.join("path"))
        .stdout(std::process::Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if Instant::now() > deadline {
            child.kill()?;
            child.wait()?;
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_tool_cache_that_cannot_be_written_is_a_miss_and_not_a_failure() -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let install = staged_install("readonly")?;
    let cache = install.work.join("cache");
    fs::create_dir_all(&cache)?;
    fs::set_permissions(&cache, fs::Permissions::from_mode(0o555))?;
    let installed = run_install(&install, &cache)?;
    fs::set_permissions(&cache, fs::Permissions::from_mode(0o755))?;
    fs::remove_dir_all(&install.work)?;
    assert!(installed);
    Ok(())
}

#[test]
fn a_cache_entry_that_is_not_a_bounded_regular_file_is_a_miss() -> TestResult {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::process::Command;

    for kind in ["fifo", "huge", "symlink"] {
        let install = staged_install(kind)?;
        let dir = install.work.join("cache/velnor-ci/1.2.3/x64");
        fs::create_dir_all(&dir)?;
        let entry = dir.join("velnor-ci");
        match kind {
            "fifo" => {
                assert!(Command::new("mkfifo").arg(&entry).status()?.success());
                fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))?;
            }
            "huge" => {
                let file = fs::File::create(&entry)?;
                file.set_len(300 * 1024 * 1024)?;
                fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))?;
            }
            _ => symlink("/bin/sh", &entry)?,
        }
        let installed = run_install(&install, &install.work.join("cache"))?;
        let from = fs::read(install.work.join("temp/velnor-ci-bin/velnor-ci")).unwrap_or_default();
        fs::remove_dir_all(&install.work)?;
        assert!(installed, "{kind}");
        assert_eq!(from, b"#!/bin/sh\necho genuine\n", "{kind}");
    }
    Ok(())
}
