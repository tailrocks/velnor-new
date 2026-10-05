//! Focused tests for generator release manifest command rendering.

use super::super::assets::{self, LINUX};
use super::{
    FILE, asset_attestation_bundle_script, attestation_bundle_script,
    manifest_attestation_bundle_script, manifest_script, publish_script,
    published_release_verify_script, tag_preflight_script,
};
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn scratch() -> Result<Scratch, Box<dyn Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "velnor-release-preflight-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory)?;
    Ok(Scratch(directory))
}

fn make_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn mock_path(scratch: &Scratch) -> Result<OsString, Box<dyn Error>> {
    let gh = scratch.0.join("gh");
    fs::write(
        &gh,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$GH_CALLS"
case "$GH_CASE" in
  confirmed-404) printf 'HTTP/2 404\r\ncontent-type: application/json\r\n\r\n{"message":"Not Found","status":"404"}\n'; exit 1 ;;
  forbidden) printf 'HTTP/2 403\r\n\r\n{"message":"Forbidden","status":"403"}\n'; exit 1 ;;
  network) exit 22 ;;
  malformed) printf 'not-an-http-response\n\nnot-json\n'; exit 1 ;;
  missing-status) printf 'HTTP/2 404\r\n\r\n{"message":"Not Found"}\n'; exit 1 ;;
esac
"#,
    )?;
    make_executable(&gh)?;
    let mise = scratch.0.join("mise");
    fs::write(
        &mise,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$MISE_CALLS"
if [ "$#" -lt 8 ] || [ "$1" != "--no-config" ] || [ "$2" != "--no-env" ] || [ "$3" != "--no-hooks" ] || [ "$4" != "exec" ] || [ "$5" != "gh@2.102.0" ] || [ "$6" != "--" ] || [ "$7" != "gh" ]; then exit 70; fi
shift 7
test -x "$MOCK_GH"
exec "$MOCK_GH" "$@"
"#,
    )?;
    make_executable(&mise)?;
    let mut paths = vec![scratch.0.clone()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").ok_or("missing PATH")?,
    ));
    Ok(std::env::join_paths(paths)?)
}

fn run_preflight(
    scratch: &Scratch,
    path: OsString,
    case: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(tag_preflight_script())
        .current_dir(workspace)
        .env("PATH", path)
        .env("GH_CALLS", scratch.0.join("calls"))
        .env("MISE_CALLS", scratch.0.join("mise-calls"))
        .env("GH_CASE", case)
        .env("MOCK_GH", scratch.0.join("gh"))
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new");
    isolate_gh_environment(&mut command, &scratch.0)?;
    Ok(command.output()?)
}

fn isolate_gh_environment(command: &mut Command, root: &Path) -> Result<(), Box<dyn Error>> {
    let home = root.join("isolated-home");
    let config = root.join("isolated-gh-config");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&config)?;
    command
        .env("HOME", home)
        .env("GH_CONFIG_DIR", config)
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_ENTERPRISE_TOKEN");
    Ok(())
}

fn assert_preflight_calls(scratch: &Scratch, accepted: bool) -> Result<(), Box<dyn Error>> {
    let calls = fs::read_to_string(scratch.0.join("calls"))?;
    let mise_calls = fs::read_to_string(scratch.0.join("mise-calls"))?;
    let count = if accepted { 2 } else { 1 };
    assert_eq!(calls.lines().count(), count);
    assert_eq!(mise_calls.lines().count(), count);
    let pinned_prefix = assets::pinned_gh_prefix();
    let pinned_args = pinned_prefix
        .strip_prefix("mise ")
        .ok_or("pinned gh command must use Mise")?;
    for (index, route) in ["git/ref/tags/v0.1.1", "releases/tags/v0.1.1"]
        .into_iter()
        .take(count)
        .enumerate()
    {
        let expected_gh = format!("api --include repos/tailrocks/velnor-new/{route}");
        assert_eq!(calls.lines().nth(index), Some(expected_gh.as_str()));
        let expected = format!("{pinned_args}api --include repos/tailrocks/velnor-new/{route}");
        assert_eq!(
            mise_calls.lines().nth(index),
            Some(expected.as_str()),
            "preflight must invoke the pinned gh through Mise: {mise_calls}"
        );
    }
    Ok(())
}

#[test]
fn manifest_bundle_uses_the_created_and_attested_file() {
    let create = manifest_script();
    let fetch = manifest_attestation_bundle_script();
    assert!(create.contains("create-release-manifest.sh"));
    assert!(fetch.contains(&format!("subject='{FILE}'")), "{fetch}");
    assert!(!fetch.contains("manifest-assets/release-manifest.json"));
}

#[test]
fn release_github_cli_commands_select_the_exact_pinned_mise_tool() {
    let scripts = [
        publish_script(),
        published_release_verify_script(),
        attestation_bundle_script(),
        asset_attestation_bundle_script(LINUX),
        manifest_attestation_bundle_script(),
    ];
    let prefix = assets::pinned_gh_prefix();
    let invocation_prefix = prefix.strip_suffix("gh ").expect("gh command suffix");
    for script in scripts {
        for command in ["gh api", "gh attestation", "gh release"] {
            for (offset, _) in script.match_indices(command) {
                assert!(
                    script[..offset].ends_with(invocation_prefix),
                    "unpinned command `{command}` in:\n{script}"
                );
            }
        }
    }
}

#[test]
fn attestation_fetch_uses_the_shell_repository_environment() {
    let expected =
        "--signer-workflow \"${GITHUB_REPOSITORY}/.github/workflows/generator-release.yml\"";
    for script in [
        asset_attestation_bundle_script(LINUX),
        manifest_attestation_bundle_script(),
    ] {
        assert!(script.contains(expected), "{script}");
        assert!(!script.contains("${{GITHUB_REPOSITORY}}"), "{script}");
    }
}

#[test]
fn tag_preflight_accepts_only_confirmed_not_found_responses() -> Result<(), Box<dyn Error>> {
    for (case, accepted) in [
        ("confirmed-404", true),
        ("forbidden", false),
        ("network", false),
        ("malformed", false),
        ("missing-status", false),
    ] {
        let scratch = scratch()?;
        let output = run_preflight(&scratch, mock_path(&scratch)?, case)?;
        assert_eq!(
            output.status.success(),
            accepted,
            "case {case}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_preflight_calls(&scratch, accepted)?;
    }
    Ok(())
}
