use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::super::{ScriptKey, ShellDialect, script_file, trusted_shell};
use super::{context, shared_fixture};

static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    output_file: String,
    environment_file: String,
    cwd_unchanged: bool,
    runner_files: Vec<(String, Vec<u8>)>,
    child_survives: bool,
}

struct CasePaths {
    root: std::path::PathBuf,
    bin: std::path::PathBuf,
    runner: std::path::PathBuf,
    output: std::path::PathBuf,
    environment: std::path::PathBuf,
    cwd: std::path::PathBuf,
    pid: std::path::PathBuf,
}

fn actual_cache_key_body() -> String {
    let shared = shared_fixture(&context());
    let step = shared
        .jobs
        .get("hosted")
        .expect("hosted job")
        .steps
        .iter()
        .find(|step| step.name == crate::mbx_bundle::MBX_CACHE_KEY_NAME)
        .expect("typed cache-key step");
    let (dialect, body) = trusted_shell(step).expect("factory-authorized shell body");
    assert_eq!(dialect, ShellDialect::Bash);
    body
}

fn run_case(body: &str, source: bool, fail_rustc: bool) -> Outcome {
    let temp = new_root().expect("create unique temporary case");
    let root = temp.path().to_path_buf();
    let paths = CasePaths {
        bin: root.join("bin"),
        runner: root.join("runner-temp"),
        output: root.join("github-output"),
        environment: root.join("github-env"),
        cwd: root.join("cwd"),
        pid: root.join("rustc-pid"),
        root,
    };
    fs::create_dir_all(&paths.bin).expect("create fake bin");
    fs::create_dir_all(&paths.runner).expect("create runner temp");
    fs::write(&paths.output, "").expect("create GITHUB_OUTPUT");
    fs::write(&paths.environment, "").expect("create GITHUB_ENV");
    install_fake_rustc(&paths.bin);

    let body_command = if source {
        let file = script_file(
            &ScriptKey {
                dialect: ShellDialect::Bash,
                body: body.to_owned(),
            },
            "0.1.0",
        )
        .expect("marked source file");
        let script_path = paths.root.join(&file.path);
        fs::create_dir_all(script_path.parent().expect("script parent"))
            .expect("create script directory");
        fs::write(&script_path, file.bytes).expect("write script file");
        format!(
            ". './{}'; printf '%s\\n' \"$PWD\" > \"$PWD_CAPTURE\"",
            file.path
        )
    } else {
        format!("{body}; printf '%s\\n' \"$PWD\" > \"$PWD_CAPTURE\"")
    };
    let monitored =
        format!("trap 'printf \"%s\\n\" \"$PWD\" > \"$PWD_CAPTURE\"' EXIT; {body_command}");
    let command = crate::toolchain_env::with_credential_unset_script(&monitored);
    let output = invoke_bash(&paths, &command, fail_rustc);
    let runner_files = files_under(&paths.runner);
    let child_survives = if paths.pid.exists() {
        child_is_alive(&paths.pid)
    } else {
        false
    };
    let outcome = Outcome {
        status: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
        output_file: fs::read_to_string(&paths.output).expect("read GITHUB_OUTPUT"),
        environment_file: fs::read_to_string(&paths.environment).expect("read GITHUB_ENV"),
        cwd_unchanged: fs::read_to_string(&paths.cwd)
            .is_ok_and(|cwd| cwd.trim_end() == paths.root.to_string_lossy()),
        runner_files,
        child_survives,
    };
    temp.cleanup()
        .expect("remove owned temporary test directory");
    outcome
}

struct OwnedTempDir {
    path: Option<PathBuf>,
    canonical: PathBuf,
}

impl OwnedTempDir {
    fn path(&self) -> &std::path::Path {
        self.path
            .as_deref()
            .expect("owned temporary directory path")
    }

    fn cleanup(mut self) -> std::io::Result<()> {
        let path = self.path();
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "temporary test path is no longer an owned directory",
            ));
        }
        if fs::canonicalize(path)? != self.canonical {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "temporary test path no longer has its original canonical identity",
            ));
        }
        fs::remove_dir_all(path)?;
        self.path = None;
        Ok(())
    }
}

impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        let Some(path) = self.path.as_deref() else {
            return;
        };
        let cleanup = (|| {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "temporary test path is no longer an owned directory",
                ));
            }
            if fs::canonicalize(path)? != self.canonical {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "temporary test path no longer has its original canonical identity",
                ));
            }
            fs::remove_dir_all(path)
        })();
        if let Err(error) = cleanup {
            eprintln!("temporary shared-script test cleanup failed: {error}");
        }
    }
}

fn new_root() -> std::io::Result<OwnedTempDir> {
    for _ in 0..128 {
        let id = NEXT_CASE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-shared-script-{}-{id}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => {
                let setup = (|| {
                    let mut permissions = fs::metadata(&path)?.permissions();
                    permissions.set_mode(0o700);
                    fs::set_permissions(&path, permissions)?;
                    let canonical = fs::canonicalize(&path)?;
                    Ok(OwnedTempDir {
                        path: Some(path.clone()),
                        canonical,
                    })
                })();
                return match setup {
                    Ok(temp) => Ok(temp),
                    Err(setup_error) => match fs::remove_dir(&path) {
                        Ok(()) => Err(setup_error),
                        Err(cleanup_error) => Err(std::io::Error::new(
                            setup_error.kind(),
                            format!(
                                "temporary directory setup failed ({setup_error}); rollback cleanup failed ({cleanup_error})"
                            ),
                        )),
                    },
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary case directory",
    ))
}

fn install_fake_rustc(bin: &std::path::Path) {
    let script = "#!/bin/sh\nprintf '%s\\n' \"$$\" > \"$CHILD_PID_CAPTURE\"\nif [ \"${GITHUB_TOKEN+x}\" = x ] || [ \"${ACTIONS_RUNTIME_TOKEN+x}\" = x ]; then exit 19; fi\nif [ \"$FAIL_RUSTC\" = 1 ]; then printf 'controlled failure\\n'; exit 7; fi\nprintf 'rustc 1.98.1\\nrelease: 1.98.1\\n'\n";
    let rustc = bin.join("rustc");
    fs::write(&rustc, script).expect("write fake rustc");
    let mut permissions = fs::metadata(&rustc).expect("stat fake rustc").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(rustc, permissions).expect("make fake rustc executable");
}

fn invoke_bash(paths: &CasePaths, command: &str, fail_rustc: bool) -> Output {
    Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(&paths.root)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", paths.bin.display()))
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("RUNNER_TEMP", &paths.runner)
        .env("GITHUB_RUN_ID", "107")
        .env("GITHUB_RUN_ATTEMPT", "2")
        .env("GITHUB_JOB", "shared-script-test")
        .env("GITHUB_OUTPUT", &paths.output)
        .env("GITHUB_ENV", &paths.environment)
        .env("PWD_CAPTURE", &paths.cwd)
        .env("CHILD_PID_CAPTURE", &paths.pid)
        .env("CACHE_REVISION", "abcdef123456")
        .env("CACHE_GENERATION", "mbx-generation")
        .env("RUST_TOOLCHAIN", "1.98.1")
        .env("CREATE_EXPORT_GROUP", "false")
        .env("FAIL_RUSTC", if fail_rustc { "1" } else { "0" })
        .env("GITHUB_TOKEN", "ambient-token")
        .env("ACTIONS_RUNTIME_TOKEN", "ambient-runtime-token")
        .output()
        .expect("run Bash case")
}

fn files_under(directory: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = fs::read_dir(directory)
        .expect("read runner temp")
        .map(|entry| {
            let entry = entry.expect("runner temp entry");
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).expect("read runner temp file"),
            )
        })
        .collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn child_is_alive(pid_path: &std::path::Path) -> bool {
    let Ok(pid) = fs::read_to_string(pid_path) else {
        return false;
    };
    Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn sourced_cache_key_matches_inline_success_and_partial_failure() {
    let body = actual_cache_key_body();
    for fail_rustc in [false, true] {
        let inline = run_case(&body, false, fail_rustc);
        let sourced = run_case(&body, true, fail_rustc);
        assert_eq!(inline, sourced);
        assert_eq!(inline.status, Some(if fail_rustc { 7 } else { 0 }));
        assert!(!inline.child_survives);
        assert!(inline.cwd_unchanged);
        if fail_rustc {
            assert!(inline.output_file.is_empty());
            assert_eq!(inline.runner_files.len(), 1);
        } else {
            assert!(inline.output_file.contains("key=linux-x64-mbx-"));
            assert!(inline.output_file.contains("prefix=linux-x64-mbx-"));
            assert!(inline.runner_files.is_empty());
        }
        assert!(inline.environment_file.is_empty());
    }
}
