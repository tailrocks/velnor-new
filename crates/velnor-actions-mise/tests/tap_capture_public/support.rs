// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    error::Error,
    ffi::OsString,
    fs, io,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use velnor_actions_mise::{BoundedCommandStatus, BoundedCommandTermination, run_command_bounded};

const CHILD_TEST: &str =
    "tap_capture_public::unix::public_capture_materializes_tracked_modes_under_restrictive_umask";

pub(super) struct TempRoot(pub(super) PathBuf);

impl TempRoot {
    pub(super) fn new() -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let path =
            env::temp_dir().join(format!("velnor-tap-capture-{}-{stamp}", std::process::id()));
        fs::create_dir(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        Ok(Self(path))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("capture fixture cleanup failed: {error}");
        }
    }
}

pub(super) struct Fixture {
    bare: PathBuf,
    working: PathBuf,
    commit: String,
    external_target: Option<PathBuf>,
}

pub(super) fn git_program() -> io::Result<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&path)
        .map(|directory| directory.join("git"))
        .find(|candidate| {
            fs::metadata(candidate).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "git executable not found"))
}

pub(super) fn invoke_git(git: &Path, cwd: Option<&Path>, args: &[OsString]) -> io::Result<Vec<u8>> {
    let mut command = Command::new(git);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS");
    run_owned(&mut command, Duration::from_secs(30))
}

pub(super) fn words(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

pub(super) fn fixture(
    root: &Path,
    git: &Path,
    include_link: bool,
) -> Result<Fixture, Box<dyn Error>> {
    let bare = root.join(if include_link {
        "linked.git"
    } else {
        "valid.git"
    });
    let working = root.join(if include_link {
        "linked-work"
    } else {
        "valid-work"
    });
    fs::create_dir(&working)?;
    let mut args = words(&["init", "--bare", "--quiet", "--template=/dev/null"]);
    args.push(bare.as_os_str().to_owned());
    invoke_git(git, None, &args)?;
    invoke_git(
        git,
        Some(&working),
        &words(&[
            "init",
            "--quiet",
            "--initial-branch=main",
            "--template=/dev/null",
        ]),
    )?;
    for (key, value) in [
        ("user.name", "Alexey Zhokhov"),
        ("user.email", "alexey@zhokhov.com"),
    ] {
        invoke_git(git, Some(&working), &words(&["config", key, value]))?;
    }

    fs::write(working.join("plain.txt"), b"plain tap bytes\n")?;
    let executable = working.join("run me.sh");
    fs::write(&executable, b"#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
    let nested = working.join("nested dir");
    fs::create_dir(&nested)?;
    fs::write(nested.join("data.bin"), [0, 1, 2, 255])?;

    let external_target = if include_link {
        let target = root.join("outside-checkout.txt");
        fs::write(&target, b"target bytes remain unchanged\n")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640))?;
        symlink(&target, working.join("external-link"))?;
        Some(target)
    } else {
        None
    };

    invoke_git(git, Some(&working), &words(&["add", "--all"]))?;
    invoke_git(
        git,
        Some(&working),
        &words(&[
            "commit",
            "--quiet",
            "-s",
            "-m",
            "fixture\n\nCo-authored-by: Codex <codex@openai.com>",
        ]),
    )?;
    let mut args = words(&["remote", "add", "origin"]);
    args.push(bare.as_os_str().to_owned());
    invoke_git(git, Some(&working), &args)?;
    invoke_git(
        git,
        Some(&working),
        &words(&["push", "--quiet", "origin", "main"]),
    )?;
    invoke_git(
        git,
        Some(&bare),
        &words(&["symbolic-ref", "HEAD", "refs/heads/main"]),
    )?;
    let output = invoke_git(git, Some(&working), &words(&["rev-parse", "HEAD"]))?;
    let commit = String::from_utf8(output)?.trim().to_owned();
    Ok(Fixture {
        bare,
        working,
        commit,
        external_target,
    })
}

pub(super) fn install_git_adapter(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let adapter_dir = root.join("adapter");
    fs::create_dir(&adapter_dir)?;
    let adapter = adapter_dir.join("git");
    fs::write(
        &adapter,
        br#"#!/bin/bash
args=()
is_tree=0
while (($#)); do
  if [[ "$1" == "-c" && "${2-}" == "protocol.file.allow=never" ]]; then
    shift 2
    continue
  fi
  if [[ "$1" == "ls-tree" ]]; then
    is_tree=1
  fi
  if [[ "$1" == "https://github.com/fixture/tap.git" ]]; then
    args+=("$TAP_CAPTURE_FIXTURE_REPOSITORY")
  else
    args+=("$1")
  fi
  shift
done
unset GIT_ALLOW_PROTOCOL
"$TAP_CAPTURE_REAL_GIT" -c protocol.file.allow=always "${args[@]}" || exit $?
if [[ "$is_tree" == 1 && -n "${TAP_CAPTURE_HARDLINK_SOURCE-}" ]]; then
  test "$PWD" = "$TAP_CAPTURE_CHECKOUT" || exit 91
  rm -- "$TAP_CAPTURE_CHECKOUT/run me.sh" || exit 92
  ln -- "$TAP_CAPTURE_HARDLINK_SOURCE" "$TAP_CAPTURE_CHECKOUT/run me.sh" || exit 93
fi
"#,
    )?;
    fs::set_permissions(&adapter, fs::Permissions::from_mode(0o755))?;
    Ok(adapter_dir)
}

pub(super) fn current_umask() -> Result<u32, Box<dyn Error>> {
    let output = run_owned(
        Command::new("/bin/sh").args(["-c", "umask"]),
        Duration::from_secs(10),
    )?;
    Ok(u32::from_str_radix(String::from_utf8(output)?.trim(), 8)?)
}

pub(super) fn run_child(
    executable: &Path,
    adapter_dir: &Path,
    real_git: &Path,
    fixture: &Fixture,
    checkout: &Path,
    scenario: &str,
    umask: &str,
) -> Result<(), Box<dyn Error>> {
    let path = env::join_paths(
        std::iter::once(adapter_dir.to_path_buf())
            .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
    )?;
    let script = format!("umask {umask}; exec \"$1\" --exact {CHILD_TEST} --nocapture");
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", &script, "tap-capture"])
        .arg(executable)
        .env("PATH", path)
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("TAP_CAPTURE_HARDLINK_SOURCE")
        .env("TAP_CAPTURE_CHILD_SCENARIO", scenario)
        .env("TAP_CAPTURE_CHILD_UMASK", umask)
        .env("TAP_CAPTURE_FIXTURE_REPOSITORY", &fixture.bare)
        .env("TAP_CAPTURE_WORKING_REPOSITORY", &fixture.working)
        .env("TAP_CAPTURE_EXPECTED_COMMIT", &fixture.commit)
        .env("TAP_CAPTURE_CHECKOUT", checkout)
        .env("TAP_CAPTURE_REAL_GIT", real_git);
    if scenario == "hardlink" {
        let parent = checkout.parent().ok_or("checkout has no parent")?;
        command.env(
            "TAP_CAPTURE_HARDLINK_SOURCE",
            parent.join("outside-hardlink.txt"),
        );
    }
    if let Some(target) = &fixture.external_target {
        command.env("TAP_CAPTURE_EXTERNAL_TARGET", target);
    }
    run_owned(&mut command, Duration::from_secs(120))
        .map(|_| ())
        .map_err(Into::into)
}

fn run_owned(command: &mut Command, timeout: Duration) -> io::Result<Vec<u8>> {
    let output = run_command_bounded(command, 1024 * 1024, timeout, Duration::from_secs(10));
    if output.status != BoundedCommandStatus::Exited(0)
        || output.termination != BoundedCommandTermination::Completed
        || output.execution_error.is_some()
        || output.cleanup_error.is_some()
    {
        return Err(io::Error::other(format!(
            "fixture command failed: {output:?}"
        )));
    }
    Ok(output.stdout)
}
