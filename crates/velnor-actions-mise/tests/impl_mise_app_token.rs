//! Real-process witness for the explicit GitHub token environment.
#![cfg(unix)]

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use velnor_actions_mise::command::PROXY_ENV_KEYS;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const CHILD_MARKER: &str = "VELNOR_GH_TOKEN_TEST_CHILD";
const CHILD_TEST: &str =
    "impl_mise_app_token::github_token_child_uses_only_the_filtered_mise_environment";

#[test]
fn github_token_child_uses_only_the_filtered_mise_environment() -> TestResult {
    if std::env::var_os(CHILD_MARKER).is_some() {
        return verify_github_token_child_environment();
    }

    let fixture = crate::test_temp_dir::TestTempDir::new("velnor-gh-token")?;
    let bin = fixture.path().join("bin");
    let home = fixture.path().join("home");
    let data = fixture.path().join("mise-data");
    std::fs::create_dir(&bin)?;
    std::fs::create_dir(&home)?;
    std::fs::create_dir(&data)?;
    write_fake_mise(&bin.join("mise"))?;
    run_github_token_child_test(fixture.path(), &bin, &home, &data)?;
    Ok(())
}

fn write_fake_mise(mise: &Path) -> TestResult {
    std::fs::write(mise, FAKE_MISE_SCRIPT)?;
    std::fs::set_permissions(mise, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

const FAKE_MISE_SCRIPT: &str = "#!/bin/sh\n\
    printf 'PATH=%s\\n' \"$PATH\"\n\
    printf 'HOME=%s\\n' \"$HOME\"\n\
    printf 'MISE_DATA_DIR=%s\\n' \"$MISE_DATA_DIR\"\n\
    printf 'GH_TOKEN=%s\\n' \"$GH_TOKEN\"\n\
    printf 'GITHUB_TOKEN=%s\\n' \"${GITHUB_TOKEN-<missing>}\"\n\
    printf 'VELNOR_TAP_TOKEN=%s\\n' \"${VELNOR_TAP_TOKEN-<missing>}\"\n\
    printf 'APP_PRIVATE_KEY=%s\\n' \"${APP_PRIVATE_KEY-<missing>}\"\n\
    printf 'HTTP_PROXY=%s\\n' \"${HTTP_PROXY-<missing>}\"\n\
    printf 'HTTPS_PROXY=%s\\n' \"${HTTPS_PROXY-<missing>}\"\n\
    printf 'NO_PROXY=%s\\n' \"${NO_PROXY-<missing>}\"\n\
    printf 'ALL_PROXY=%s\\n' \"${ALL_PROXY-<missing>}\"\n\
    printf 'http_proxy=%s\\n' \"${http_proxy-<missing>}\"\n\
    printf 'https_proxy=%s\\n' \"${https_proxy-<missing>}\"\n\
    printf 'no_proxy=%s\\n' \"${no_proxy-<missing>}\"\n\
    printf 'all_proxy=%s\\n' \"${all_proxy-<missing>}\"\n\
    printf 'GH_HOST=%s\\n' \"${GH_HOST-<missing>}\"\n\
    printf 'GH_CONFIG_DIR=%s\\n' \"${GH_CONFIG_DIR-<missing>}\"\n\
    printf 'MISE_CONFIG_FILE=%s\\n' \"${MISE_CONFIG_FILE-<missing>}\"\n\
    printf 'MISE_ENV=%s\\n' \"${MISE_ENV-<missing>}\"\n\
    printf 'MISE_TRUSTED_CONFIG_PATHS=%s\\n' \"${MISE_TRUSTED_CONFIG_PATHS-<missing>}\"\n\
    printf 'MISE_RUSTUP_HOME=%s\\n' \"${MISE_RUSTUP_HOME-<missing>}\"\n\
    printf 'MISE_CARGO_HOME=%s\\n' \"${MISE_CARGO_HOME-<missing>}\"\n\
    printf 'XDG_CONFIG_HOME=%s\\n' \"${XDG_CONFIG_HOME-<missing>}\"\n\
    printf 'MISE_GITHUB_TOKEN=%s\\n' \"${MISE_GITHUB_TOKEN-<missing>}\"\n\
    printf 'CARGO_REGISTRIES_CRATES_IO_TOKEN=%s\\n' \"${CARGO_REGISTRIES_CRATES_IO_TOKEN-<missing>}\"\n\
    printf 'MISE_NO_CONFIG=%s\\n' \"$MISE_NO_CONFIG\"\n\
    printf 'MISE_NO_ENV=%s\\n' \"$MISE_NO_ENV\"\n\
    printf 'MISE_NO_HOOKS=%s\\n' \"$MISE_NO_HOOKS\"\n\
    printf 'MISE_LOCKFILE=%s\\n' \"$MISE_LOCKFILE\"\n\
    printf 'MISE_AUTO_INSTALL=%s\\n' \"$MISE_AUTO_INSTALL\"\n\
    printf 'MISE_EXEC_AUTO_INSTALL=%s\\n' \"$MISE_EXEC_AUTO_INSTALL\"\n\
    printf 'ARG=%s\\n' \"$@\"\n";

fn run_github_token_child_test(root: &Path, bin: &Path, home: &Path, data: &Path) -> TestResult {
    let child = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", CHILD_TEST, "--nocapture"])
        .env_clear()
        .env(CHILD_MARKER, "1")
        .env("PATH", bin)
        .env("HOME", home)
        .env("MISE_DATA_DIR", data)
        .env("GH_TOKEN", "ambient-token-bait")
        .env("GITHUB_TOKEN", "ambient-token-bait")
        .env("VELNOR_TAP_TOKEN", "app-token-bait")
        .env("APP_PRIVATE_KEY", "private-key-bait")
        .env("GH_HOST", "attacker.invalid")
        .env("GH_CONFIG_DIR", root.join("poison-gh"))
        .env("MISE_CONFIG_FILE", root.join("poison-config"))
        .env("MISE_ENV", "poison")
        .env("MISE_TRUSTED_CONFIG_PATHS", root)
        .env("MISE_RUSTUP_HOME", root.join("poison-rustup"))
        .env("MISE_CARGO_HOME", root.join("poison-cargo"))
        .env("XDG_CONFIG_HOME", root.join("poison-xdg"))
        .env("MISE_GITHUB_TOKEN", "mise-token-bait")
        .env("CARGO_REGISTRIES_CRATES_IO_TOKEN", "registry-token-bait")
        .envs(PROXY_ENV_KEYS.map(|key| (key, "proxy-bait")))
        .output()?;
    assert!(
        child.status.success(),
        "child test failed: {}",
        String::from_utf8_lossy(&child.stderr)
    );
    Ok(())
}

fn verify_github_token_child_environment() -> TestResult {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        vec![OsString::from("api"), OsString::from("user")],
    )?;
    let command =
        exec.command_with_github_token(&ToolCatalog::pinned(), "ghs_selected_test_token")?;
    let output = command.run()?;
    assert!(
        output.success,
        "fake mise failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout)?;
    let lines: Vec<_> = text.lines().collect();
    let expected_env = expected_child_environment(&command)?;
    let observed_env: Vec<_> = lines[..expected_env.len()]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
    assert_eq!(observed_env, expected_env);
    let expected_argv: Vec<_> = command
        .argv()
        .iter()
        .skip(1)
        .map(|arg| format!("ARG={}", arg.to_string_lossy()))
        .collect();
    let observed_argv: Vec<_> = lines[expected_env.len()..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
    assert_eq!(observed_argv, expected_argv);
    Ok(())
}

fn expected_child_environment(
    command: &velnor_actions_mise::IsolatedCommand,
) -> TestResult<Vec<String>> {
    let expected = vec![
        format!("PATH={}", std::env::var("PATH")?),
        format!("HOME={}", std::env::var("HOME")?),
        format!("MISE_DATA_DIR={}", std::env::var("MISE_DATA_DIR")?),
        "GH_TOKEN=ghs_selected_test_token".to_owned(),
        "GITHUB_TOKEN=<missing>".to_owned(),
        "VELNOR_TAP_TOKEN=<missing>".to_owned(),
        "APP_PRIVATE_KEY=<missing>".to_owned(),
        "HTTP_PROXY=<missing>".to_owned(),
        "HTTPS_PROXY=<missing>".to_owned(),
        "NO_PROXY=<missing>".to_owned(),
        "ALL_PROXY=<missing>".to_owned(),
        "http_proxy=<missing>".to_owned(),
        "https_proxy=<missing>".to_owned(),
        "no_proxy=<missing>".to_owned(),
        "all_proxy=<missing>".to_owned(),
        "GH_HOST=<missing>".to_owned(),
        "GH_CONFIG_DIR=<missing>".to_owned(),
        "MISE_CONFIG_FILE=<missing>".to_owned(),
        "MISE_ENV=<missing>".to_owned(),
        "MISE_TRUSTED_CONFIG_PATHS=<missing>".to_owned(),
        "MISE_RUSTUP_HOME=<missing>".to_owned(),
        "MISE_CARGO_HOME=<missing>".to_owned(),
        "XDG_CONFIG_HOME=<missing>".to_owned(),
        "MISE_GITHUB_TOKEN=<missing>".to_owned(),
        "CARGO_REGISTRIES_CRATES_IO_TOKEN=<missing>".to_owned(),
        "MISE_NO_CONFIG=1".to_owned(),
        "MISE_NO_ENV=1".to_owned(),
        "MISE_NO_HOOKS=1".to_owned(),
        "MISE_LOCKFILE=0".to_owned(),
        "MISE_AUTO_INSTALL=false".to_owned(),
        "MISE_EXEC_AUTO_INSTALL=false".to_owned(),
    ];
    let command_env: Vec<_> = command
        .full_env()
        .iter()
        .map(|(key, value)| format!("{}={}", key.to_string_lossy(), value.to_string_lossy()))
        .collect();
    assert_eq!(
        command_env.join("\n"),
        "MISE_NO_CONFIG=1\nMISE_NO_ENV=1\nMISE_NO_HOOKS=1\nMISE_LOCKFILE=0\nMISE_AUTO_INSTALL=false\nMISE_EXEC_AUTO_INSTALL=false\nGH_TOKEN=ghs_selected_test_token",
        "command carries only its fixed policy and selected credential"
    );
    Ok(expected)
}
