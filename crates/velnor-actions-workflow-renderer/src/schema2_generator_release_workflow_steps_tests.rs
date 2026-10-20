use super::{Yaml, gh_confirmation_function, gh_function, gh_function_with_timeout};
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

#[test]
fn confirmation_deadline_is_inherited_and_clamps_inflight_cleanup() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let mock_bin = scratch.0.join("mock-bin");
    fs::create_dir(&mock_bin)?;
    write_executable(
        &mock_bin.join("mise"),
        "#!/bin/sh\ntrap '' TERM\nexec sleep 30\n",
    )?;
    let function = gh_confirmation_function(&pinned_gh_argv())?;
    let script = format!(
        "{function}\ndeadline=$((SECONDS + 7)); sleep 1\nparent_seconds=$SECONDS\n(( parent_seconds >= 1 )) || exit 99\n( test \"$SECONDS\" -ge \"$parent_seconds\" ) || exit 99\ngh_until \"$deadline\" release verify fixture"
    );
    let started = std::time::Instant::now();
    let output = Command::new("bash")
        .args(["-e", "-c", &script])
        .env("PATH", path_with(&mock_bin)?)
        .output()?;
    assert_eq!(output.status.code(), Some(124));
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert!(started.elapsed() >= std::time::Duration::from_secs(6));
    Ok(())
}

#[test]
fn expired_confirmation_deadline_never_invokes_gh() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let mock_bin = scratch.0.join("mock-bin");
    fs::create_dir(&mock_bin)?;
    let marker = scratch.0.join("invoked");
    write_executable(
        &mock_bin.join("mise"),
        &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )?;
    let function = gh_confirmation_function(&pinned_gh_argv())?;
    let output = Command::new("bash")
        .args([
            "-e",
            "-c",
            &format!("{function}\ngh_until \"$SECONDS\" release verify fixture"),
        ])
        .env("PATH", path_with(&mock_bin)?)
        .output()?;
    assert_eq!(output.status.code(), Some(124));
    assert!(!marker.exists());
    Ok(())
}

#[test]
fn mise_install_requires_the_scoped_workflow_token_before_invocation() -> Result<(), Box<dyn Error>>
{
    let step = super::mise_install_step(
        "Install pinned Rust and MBX",
        &[
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "install".to_owned(),
            "mr-boxington@1.25.0".to_owned(),
        ],
    )?;
    let Yaml::Map(fields) = step else {
        return Err("Mise install must render a step mapping".into());
    };
    let field = |name: &str| {
        fields
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value))
    };
    let Yaml::Map(env) = field("env").ok_or("Mise install has no step environment")? else {
        return Err("Mise install environment must be a mapping".into());
    };
    assert_eq!(
        env,
        &[("GITHUB_TOKEN".to_owned(), Yaml::str("${{ github.token }}"))]
    );
    let Yaml::Str(run) = field("run").ok_or("Mise install has no command")? else {
        return Err("Mise install command must be a string".into());
    };
    assert!(
        run.starts_with("set -eu\n: \"${GITHUB_TOKEN:?missing workflow token}\"\n"),
        "token guard must precede the install command"
    );
    assert!(run.contains("mise --no-config --no-env --no-hooks install mr-boxington@1.25.0"));

    let scratch = Scratch::new()?;
    let mock_bin = scratch.0.join("mock-bin");
    fs::create_dir(&mock_bin)?;
    let invoked = scratch.0.join("mise-invoked");
    write_executable(
        &mock_bin.join("mise"),
        &format!("#!/bin/sh\n: > '{}'\n", invoked.display()),
    )?;
    let output = Command::new("bash")
        .args(["-c", run])
        .env("PATH", path_with(&mock_bin)?)
        .env("GITHUB_TOKEN", "")
        .output()?;
    assert!(!output.status.success(), "empty token unexpectedly passed");
    assert!(!invoked.exists(), "Mise ran without its workflow token");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("missing workflow token"),
        "missing-token failure must be explicit without printing credentials"
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
