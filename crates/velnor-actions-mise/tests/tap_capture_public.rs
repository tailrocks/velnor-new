// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
mod unix {
    use std::{
        env,
        error::Error,
        ffi::OsString,
        fs, io,
        os::unix::{fs::MetadataExt, fs::PermissionsExt, fs::symlink},
        path::{Path, PathBuf},
        process::{Command, Output},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    use velnor_actions_mise::{CheckDeadline, capture_tap_main, read_tap_tree};

    const CHILD_TEST: &str = "tap_capture_public::unix::public_capture_materializes_tracked_modes_under_restrictive_umask";

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> io::Result<Self> {
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

    struct Fixture {
        bare: PathBuf,
        working: PathBuf,
        commit: String,
        external_target: Option<PathBuf>,
    }

    fn git_program() -> io::Result<PathBuf> {
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

    fn invoke_git(git: &Path, cwd: Option<&Path>, args: &[OsString]) -> io::Result<Output> {
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
        let output = command.output()?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(io::Error::other(format!(
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    fn words(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn fixture(root: &Path, git: &Path, include_link: bool) -> Result<Fixture, Box<dyn Error>> {
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
        invoke_git(
            git,
            Some(&working),
            &words(&["config", "user.name", "Tap Fixture"]),
        )?;
        invoke_git(
            git,
            Some(&working),
            &words(&["config", "user.email", "tap@example.invalid"]),
        )?;

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
            &words(&["commit", "--quiet", "-m", "fixture"]),
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
        let commit = String::from_utf8(output.stdout)?.trim().to_owned();
        Ok(Fixture {
            bare,
            working,
            commit,
            external_target,
        })
    }

    fn install_git_adapter(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
        let adapter_dir = root.join("adapter");
        fs::create_dir(&adapter_dir)?;
        let adapter = adapter_dir.join("git");
        fs::write(
            &adapter,
            b"#!/bin/bash\nargs=()\nwhile (($#)); do\n  if [[ \"$1\" == \"-c\" && \"${2-}\" == \"protocol.file.allow=never\" ]]; then\n    shift 2\n    continue\n  fi\n  if [[ \"$1\" == \"https://github.com/fixture/tap.git\" ]]; then\n    args+=(\"$TAP_CAPTURE_FIXTURE_REPOSITORY\")\n  else\n    args+=(\"$1\")\n  fi\n  shift\ndone\nunset GIT_ALLOW_PROTOCOL\nexec \"$TAP_CAPTURE_REAL_GIT\" -c protocol.file.allow=always \"${args[@]}\"\n",
        )?;
        fs::set_permissions(&adapter, fs::Permissions::from_mode(0o755))?;
        Ok(adapter_dir)
    }

    fn current_umask() -> Result<u32, Box<dyn Error>> {
        let output = Command::new("/bin/sh").args(["-c", "umask"]).output()?;
        if !output.status.success() {
            return Err("could not read child umask".into());
        }
        Ok(u32::from_str_radix(
            String::from_utf8(output.stdout)?.trim(),
            8,
        )?)
    }

    fn run_child(
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
            .env("TAP_CAPTURE_CHILD_SCENARIO", scenario)
            .env("TAP_CAPTURE_CHILD_UMASK", umask)
            .env("TAP_CAPTURE_FIXTURE_REPOSITORY", &fixture.bare)
            .env("TAP_CAPTURE_WORKING_REPOSITORY", &fixture.working)
            .env("TAP_CAPTURE_EXPECTED_COMMIT", &fixture.commit)
            .env("TAP_CAPTURE_CHECKOUT", checkout)
            .env("TAP_CAPTURE_REAL_GIT", real_git);
        if let Some(target) = &fixture.external_target {
            command.env("TAP_CAPTURE_EXTERNAL_TARGET", target);
        }
        let output = command.output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "child regression failed scenario={scenario} umask={umask}; stdout: {}; stderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ))
            .into())
        }
    }

    fn physical_mode(path: &Path) -> io::Result<u32> {
        Ok(fs::metadata(path)?.permissions().mode() & 0o777)
    }

    fn hardlink_scenario(checkout: &Path, deadline: CheckDeadline) -> Result<(), Box<dyn Error>> {
        capture_tap_main("fixture/tap", checkout, deadline)?;
        let tracked = checkout.join("plain.txt");
        let external = checkout
            .parent()
            .ok_or("checkout has no parent")?
            .join("outside-hardlink.txt");
        fs::hard_link(&tracked, &external)?;
        fs::set_permissions(&tracked, fs::Permissions::from_mode(0o600))?;
        let original_bytes = fs::read(&external)?;
        assert_eq!(physical_mode(&external)?, 0o600);
        let result = capture_tap_main("fixture/tap", checkout, deadline);
        assert_eq!(fs::read(&external)?, original_bytes);
        assert_eq!(physical_mode(&external)?, 0o600);
        if result.is_ok() {
            let external_metadata = fs::metadata(&external)?;
            let tracked_metadata = fs::metadata(&tracked)?;
            assert_ne!(
                (tracked_metadata.dev(), tracked_metadata.ino()),
                (external_metadata.dev(), external_metadata.ino())
            );
        }
        Ok(())
    }

    #[test]
    fn public_capture_materializes_tracked_modes_under_restrictive_umask()
    -> Result<(), Box<dyn Error>> {
        if let Ok(scenario) = env::var("TAP_CAPTURE_CHILD_SCENARIO") {
            return child_scenario(&scenario);
        }
        let root = TempRoot::new()?;
        let git = git_program()?;
        let valid = fixture(&root.0, &git, false)?;
        let linked = fixture(&root.0, &git, true)?;
        let adapter = install_git_adapter(&root.0)?;
        let executable = env::current_exe()?;

        run_child(
            &executable,
            &adapter,
            &git,
            &valid,
            &root.0.join("checkout-022"),
            "modes",
            "022",
        )?;
        println!("tap_capture_control=pass umask=022");
        run_child(
            &executable,
            &adapter,
            &git,
            &valid,
            &root.0.join("checkout-077"),
            "modes",
            "077",
        )?;
        run_child(
            &executable,
            &adapter,
            &git,
            &valid,
            &root.0.join("checkout-hardlink"),
            "hardlink",
            "077",
        )?;
        run_child(
            &executable,
            &adapter,
            &git,
            &linked,
            &root.0.join("checkout-link"),
            "link",
            "077",
        )?;
        Ok(())
    }

    fn child_scenario(scenario: &str) -> Result<(), Box<dyn Error>> {
        let expected_umask = u32::from_str_radix(&env::var("TAP_CAPTURE_CHILD_UMASK")?, 8)?;
        let commit = env::var("TAP_CAPTURE_EXPECTED_COMMIT")?;
        let checkout =
            PathBuf::from(env::var_os("TAP_CAPTURE_CHECKOUT").ok_or("missing checkout")?);
        fs::create_dir(&checkout)?;
        fs::set_permissions(&checkout, fs::Permissions::from_mode(0o700))?;
        let deadline = CheckDeadline::after(Duration::from_secs(90))?;

        match scenario {
            "link" => {
                let result = capture_tap_main("fixture/tap", &checkout, deadline);
                assert!(result.is_err());
                let tree = read_tap_tree(
                    &env::var_os("TAP_CAPTURE_WORKING_REPOSITORY")
                        .map(PathBuf::from)
                        .ok_or("missing working repository")?,
                    &commit,
                    deadline,
                );
                assert!(tree.is_err());
                let target = PathBuf::from(
                    env::var_os("TAP_CAPTURE_EXTERNAL_TARGET").ok_or("missing external target")?,
                );
                assert_eq!(fs::read(&target)?, b"target bytes remain unchanged\n");
                assert_eq!(physical_mode(&target)?, 0o640);
            }
            "modes" => {
                let captured = capture_tap_main("fixture/tap", &checkout, deadline)?;
                assert_eq!(captured.commit_sha, commit);
                assert_eq!(captured.checkout, checkout);
                let entries = read_tap_tree(&checkout, &commit, deadline)?;
                for (path, mode) in [
                    ("nested dir/data.bin", 0o100_644),
                    ("plain.txt", 0o100_644),
                    ("run me.sh", 0o100_755),
                ] {
                    assert!(
                        entries
                            .iter()
                            .any(|entry| entry.path == path && entry.mode == mode)
                    );
                }
                for (path, bytes, mode) in [
                    ("nested dir/data.bin", &[0, 1, 2, 255][..], 0o644),
                    ("plain.txt", &b"plain tap bytes\n"[..], 0o644),
                    ("run me.sh", &b"#!/bin/sh\nexit 0\n"[..], 0o755),
                ] {
                    let file = checkout.join(path);
                    assert_eq!(fs::read(&file)?, bytes);
                    assert_eq!(physical_mode(&file)?, mode);
                }
                let git = PathBuf::from(env::var_os("TAP_CAPTURE_REAL_GIT").ok_or("missing git")?);
                let head = invoke_git(&git, Some(&checkout), &words(&["rev-parse", "HEAD"]))?;
                assert_eq!(String::from_utf8(head.stdout)?.trim(), commit);
                let status = invoke_git(
                    &git,
                    Some(&checkout),
                    &words(&["status", "--porcelain=v1", "-z", "--untracked-files=all"]),
                )?;
                assert_eq!(status.stdout, Vec::<u8>::new());
            }
            "hardlink" => {
                hardlink_scenario(&checkout, deadline)?;
            }
            _ => return Err(format!("unknown capture scenario: {scenario}").into()),
        }

        assert_eq!(current_umask()?, expected_umask);
        Ok(())
    }
}
