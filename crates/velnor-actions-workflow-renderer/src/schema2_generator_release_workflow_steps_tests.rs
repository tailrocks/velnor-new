use super::{gh_function, gh_function_with_timeout};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path =
            std::env::temp_dir().join(format!("velnor-pinned-gh-{}-{nonce}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[test]
fn typed_gh_function_shadows_incompatible_cli_and_exports_to_script() -> Result<(), Box<dyn Error>>
{
    let scratch = Scratch::new()?;
    let mock_bin = scratch.0.join("mock-bin");
    fs::create_dir(&mock_bin)?;
    let system_gh_called = scratch.0.join("system-gh-called");
    write_executable(
        &mock_bin.join("gh"),
        &format!(
            "#!/bin/sh\nprintf system-gh > '{}'\nexit 90\n",
            system_gh_called.display()
        ),
    )?;
    let mise_log = scratch.0.join("mise-argv");
    write_executable(
        &mock_bin.join("mise"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$*\" in *'gh api fail-status'*) exit 37 ;; esac\nprintf 'pinned-gh:%s\\n' \"$*\"\n",
            mise_log.display()
        ),
    )?;
    let timeout_called = scratch.0.join("timeout-called");
    write_executable(
        &mock_bin.join("timeout"),
        &format!(
            "#!/bin/sh\nprintf invoked > '{}'\nexit 99\n",
            timeout_called.display()
        ),
    )?;
    let function = gh_function(&pinned_gh_argv())?;
    for command in ["gh api test", "bash -c 'gh api nested'"] {
        let script = format!("{function}\n{command}");
        let output = Command::new("bash")
            .args(["-e", "-c", &script])
            .env("PATH", path_with(&mock_bin)?)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout)?.starts_with("pinned-gh:"));
    }
    assert!(
        !system_gh_called.exists(),
        "used the incompatible system gh"
    );
    let invocation = fs::read_to_string(mise_log)?;
    assert!(invocation.contains("--no-config --no-env --no-hooks exec gh@2.102.0 -- gh api test"));
    assert!(
        invocation.contains("--no-config --no-env --no-hooks exec gh@2.102.0 -- gh api nested")
    );
    assert!(
        !timeout_called.exists(),
        "wrapper invoked GNU timeout, which macOS runners lack"
    );
    let failure = Command::new("bash")
        .args(["-e", "-c", &format!("{function}\ngh api fail-status")])
        .env("PATH", path_with(&mock_bin)?)
        .output()?;
    assert_eq!(failure.status.code(), Some(37));
    Ok(())
}

#[test]
fn bounded_gh_function_terminates_a_hung_request() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let mock_bin = scratch.0.join("mock-bin");
    fs::create_dir(&mock_bin)?;
    write_executable(&mock_bin.join("mise"), "#!/bin/sh\nexec sleep 30\n")?;
    write_executable(
        &mock_bin.join("timeout"),
        "#!/bin/sh\necho 'poison: timeout must never be invoked' >&2\nexit 99\n",
    )?;
    let function = gh_function_with_timeout(&pinned_gh_argv(), 1)?;
    assert!(
        function.contains("( sleep 1; kill -TERM") && function.contains("sleep 5; kill -KILL"),
        "watchdog lost its TERM-then-KILL budget: {function}"
    );
    let started = std::time::Instant::now();
    let output = Command::new("bash")
        .args(["-e", "-c", &format!("{function}\ngh api hang")])
        .env("PATH", path_with(&mock_bin)?)
        .output()?;

    assert!(
        !output.status.success(),
        "hung request unexpectedly succeeded"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(8),
        "hung request exceeded its timeout plus kill grace"
    );
    assert!(
        started.elapsed() >= std::time::Duration::from_secs(1),
        "hung request returned before its one-second budget"
    );
    Ok(())
}

fn pinned_gh_argv() -> Vec<String> {
    [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "gh@2.102.0",
        "--",
        "gh",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn write_executable(path: &std::path::Path, contents: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn path_with(mock_bin: &std::path::Path) -> Result<std::ffi::OsString, Box<dyn Error>> {
    let mut paths = vec![mock_bin.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").ok_or("missing PATH")?,
    ));
    Ok(std::env::join_paths(paths)?)
}
