// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_render::SETUP_VELNOR_ACTION;

pub(crate) type TestResult = Result<(), Box<dyn Error>>;

const REPOSITORY: &str = "tailrocks/velnor-new/";
const INSTALL_STEP: &str = "      - name: Install velnor-ci";
const TOOLS: &str = "/usr/bin:/bin";

pub(crate) fn program(name: &str) -> String {
    ["/usr/bin", "/bin"]
        .iter()
        .map(|dir| format!("{dir}/{name}"))
        .find(|path| Path::new(path).exists())
        .unwrap_or_else(|| name.to_owned())
}

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
    let digest = Command::new(program("shasum"))
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
    let status = Command::new(program("bash"))
        .arg(&script)
        .env("VERSION", "1.2.3")
        .env("SHA256_LINUX_X64", digest)
        .env("SHA256_LINUX_ARM64", "")
        .env("RELEASES", format!("file://{}", releases.display()))
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("RUNNER_TOOL_CACHE", &cache)
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_PATH", &path_file)
        .env("PATH", TOOLS)
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
    assert_ne!(script, "");
    assert!(
        !script.contains("${{"),
        "inputs must reach the script through env"
    );
    Ok(())
}

pub(crate) struct Install {
    pub(crate) work: std::path::PathBuf,
    pub(crate) binary: &'static [u8],
}

impl Install {
    pub(crate) fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let work = std::env::temp_dir().join(format!("velnor-gen-{name}-{}", std::process::id()));
        let release = work.join("releases/rust-ci-v1.2.3");
        fs::create_dir_all(&release)?;
        fs::create_dir_all(work.join("temp"))?;
        fs::write(release.join("velnor-ci-linux-x64"), Self::BINARY)?;
        Ok(Self {
            work,
            binary: Self::BINARY,
        })
    }

    const BINARY: &'static [u8] = b"#!/bin/sh\necho genuine\n";

    pub(crate) fn run(
        &self,
        cache: Option<&Path>,
        shim_dir: Option<&Path>,
    ) -> Result<std::process::ExitStatus, Box<dyn Error>> {
        let mut child = self.spawn_with(cache, shim_dir, |_| {})?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            if std::time::Instant::now() >= deadline {
                child.kill().ok();
                child.wait().ok();
                return Err("the install script did not finish in 30 seconds".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    pub(crate) fn spawn(
        &self,
        cache: Option<&Path>,
        configure: impl FnOnce(&mut std::process::Command),
    ) -> Result<std::process::Child, Box<dyn Error>> {
        self.spawn_with(cache, None, configure)
    }

    pub(crate) fn spawn_with(
        &self,
        cache: Option<&Path>,
        shim_dir: Option<&Path>,
        configure: impl FnOnce(&mut std::process::Command),
    ) -> Result<std::process::Child, Box<dyn Error>> {
        use std::process::Command;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let action = fs::read_to_string(root.join("actions/setup-velnor/action.yml"))?;
        let digest = Command::new(program("shasum"))
            .args(["-a", "256"])
            .arg(
                self.work
                    .join("releases/rust-ci-v1.2.3/velnor-ci-linux-x64"),
            )
            .output()?
            .stdout;
        let digest = String::from_utf8(digest)?;
        let digest = digest.split_whitespace().next().ok_or("no digest")?;
        let script = self.work.join("install.sh");
        fs::write(&script, install_script(&action))?;
        let mut command = Command::new(program("bash"));
        command
            .arg(&script)
            .env("VERSION", "1.2.3")
            .env("SHA256_LINUX_X64", digest)
            .env("SHA256_LINUX_ARM64", "")
            .env(
                "RELEASES",
                format!("file://{}", self.work.join("releases").display()),
            )
            .env("RUNNER_OS", "Linux")
            .env("RUNNER_ARCH", "X64")
            .env("RUNNER_TEMP", self.work.join("temp"))
            .env("GITHUB_PATH", self.work.join("path"))
            .env("PATH", TOOLS)
            .env_remove("RUNNER_TOOL_CACHE");
        if let Some(cache) = cache {
            command.env("RUNNER_TOOL_CACHE", cache);
        }
        if let Some(shims) = shim_dir {
            command.env("PATH", format!("{}:{TOOLS}", shims.display()));
        }
        configure(&mut command);
        Ok(command.spawn()?)
    }

    pub(crate) fn installed(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let on_path = fs::read_to_string(self.work.join("path"))?;
        Ok(fs::read(Path::new(on_path.trim()).join("velnor-ci"))?)
    }
}
