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
        .env("SHA256_MACOS_ARM64", "")
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
        Ok(child.wait()?)
    }

    fn spawn(
        &self,
        cache: Option<&Path>,
        configure: impl FnOnce(&mut std::process::Command),
    ) -> Result<std::process::Child, Box<dyn Error>> {
        self.spawn_with(cache, None, configure)
    }

    fn spawn_with(
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
            .env("SHA256_MACOS_ARM64", "")
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

fn read_only_cache_installs_privately(install: &Install, leaf: &str) -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let cache = install.work.join(leaf.split('/').next().unwrap_or(leaf));
    let locked = install.work.join(leaf);
    fs::create_dir_all(&locked)?;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555))?;
    if fs::write(locked.join("probe"), b"").is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))?;
        return Ok(());
    }
    let status = install.run(Some(&cache), None)?;
    let copy = install.installed();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))?;
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert_eq!(copy?, install.binary);
    Ok(())
}

#[test]
fn an_unset_tool_cache_does_not_stop_the_private_install() -> TestResult {
    let install = Install::new("cache-unset")?;
    let status = install.run(None, None)?;
    let copy = install.installed();
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert_eq!(copy?, install.binary);
    Ok(())
}

#[test]
fn a_read_only_tool_cache_does_not_stop_the_private_install() -> TestResult {
    read_only_cache_installs_privately(&Install::new("cache-read-only")?, "read-only")
}

#[test]
fn a_read_only_cache_entry_directory_does_not_stop_the_private_install() -> TestResult {
    read_only_cache_installs_privately(
        &Install::new("cache-read-only-leaf")?,
        "read-only-leaf/velnor-ci/1.2.3/x64",
    )
}

pub(crate) fn seed_entry(
    install: &Install,
) -> Result<(std::path::PathBuf, std::path::PathBuf), Box<dyn Error>> {
    let cache = install.work.join("cache");
    let entry = cache.join("velnor-ci/1.2.3/x64");
    fs::create_dir_all(&entry)?;
    Ok((cache, entry.join("velnor-ci")))
}

#[test]
fn a_cache_entry_that_is_not_a_regular_file_is_ignored_instead_of_read() -> TestResult {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    for kind in ["fifo", "symlink-to-fifo"] {
        let install = Install::new(&format!("cache-{kind}"))?;
        let (cache, entry) = seed_entry(&install)?;
        let fifo = if kind == "fifo" {
            entry.clone()
        } else {
            install.work.join("elsewhere")
        };
        assert!(
            Command::new(program("mkfifo"))
                .arg(&fifo)
                .status()?
                .success()
        );
        fs::set_permissions(&fifo, fs::Permissions::from_mode(0o755))?;
        if kind != "fifo" {
            symlink(&fifo, &entry)?;
        }
        let mut child = install.spawn(Some(&cache), |command| {
            command.process_group(0);
        })?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if std::time::Instant::now() > deadline {
                let group = child.id().to_string();
                Command::new(program("kill"))
                    .args(["-KILL", "--", &format!("-{group}")])
                    .status()?;
                child.wait()?;
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let copy = install.installed();
        fs::remove_dir_all(&install.work)?;
        assert!(
            status.is_some_and(|status| status.success()),
            "{kind}: the install hung or failed"
        );
        assert_eq!(copy?, install.binary, "{kind}");
    }
    Ok(())
}

#[test]
fn a_tampered_cache_entry_is_rejected_by_the_sha256_and_replaced_by_a_download() -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let install = Install::new("cache-tampered")?;
    let (cache, entry) = seed_entry(&install)?;
    fs::write(&entry, b"#!/bin/sh\necho tampered\n")?;
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))?;
    let status = install.run(Some(&cache), None)?;
    let copy = install.installed();
    let republished = fs::read(&entry);
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert_eq!(copy?, install.binary);
    assert_eq!(republished?, install.binary);
    Ok(())
}

#[test]
fn a_failed_copy_into_the_tool_cache_publishes_no_entry() -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let install = Install::new("cache-copy-fails")?;
    let shims = install.work.join("shims");
    fs::create_dir_all(&shims)?;
    let cp = shims.join("cp");
    fs::write(
        &cp,
        "#!/bin/sh\ncase \"$2\" in\n  */.velnor-ci.*) head -c 5 \"$1\" > \"$2\"; exit 1 ;;\nesac\nexec /bin/cp \"$@\"\n",
    )?;
    fs::set_permissions(&cp, fs::Permissions::from_mode(0o755))?;
    let cache = install.work.join("cache");
    let status = install.run(Some(&cache), Some(&shims))?;
    let entries: Vec<_> = fs::read_dir(cache.join("velnor-ci/1.2.3/x64"))?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<Result<_, _>>()?;
    let copy = install.installed();
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert!(entries.is_empty(), "{entries:?}");
    assert_eq!(copy?, install.binary);
    Ok(())
}
