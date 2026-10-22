//! Test the generated verifier with local command fixtures.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_mise::catalog::RUST_VERSION;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct ShellFixture {
    temp: TempDir,
    rustup: PathBuf,
    bin: PathBuf,
}

impl ShellFixture {
    fn new(catalog: &ToolCatalog, installed: Option<&str>) -> std::io::Result<Self> {
        let temp = TempDir::new()?;
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin)?;
        let rustup = temp.path().join("rustup");
        let manifest = rustup
            .join("toolchains")
            .join(catalog.rust_toolchain_name())
            .join("lib/rustlib/multirust-channel-manifest.toml");
        if let Some(contents) = installed {
            fs::create_dir_all(
                manifest
                    .parent()
                    .expect("The installed manifest path must have a parent directory."),
            )?;
            fs::write(&manifest, contents)?;
        }

        let hash_tool = Command::new("sh")
            .args(["-c", "command -v sha256sum || command -v shasum"])
            .output()?;
        if !hash_tool.status.success() {
            return Err(std::io::Error::other(
                "The test could not find a SHA-256 utility.",
            ));
        }
        write_executable(
            &bin.join("curl"),
            r#"#!/bin/sh
out=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) out=$2; shift 2 ;;
    *) shift ;;
  esac
done
[ -n "$out" ] || exit 2
case "${VELNOR_TEST_URL-}" in
  https://static.rust-lang.org/dist/channel-rust-*.toml) ;;
  *) exit 3 ;;
esac
printf '%s' "${VELNOR_TEST_CHANNEL-}" > "$out"
printf called > "$RUNNER_TEMP/curl-called"
"#,
        )?;
        write_executable(
            &bin.join("sha256sum"),
            r#"#!/bin/sh
if [ "${1-}" = -c ] && [ "${VELNOR_TEST_SKIP_DIGEST-}" = yes ]; then exit 0; fi
if [ "$VELNOR_TEST_HASH_KIND" = shasum ]; then
  exec "$VELNOR_TEST_HASH_TOOL" -a 256 "$@"
fi
exec "$VELNOR_TEST_HASH_TOOL" "$@"
"#,
        )?;
        write_executable(
            &bin.join("grep"),
            r#"#!/bin/sh
case "${VELNOR_TEST_GREP_FAILURE-}:$*" in
  channel:*channel.toml*) /usr/bin/grep "$@"; exit 7 ;;
  installed:*multirust-channel-manifest.toml*) /usr/bin/grep "$@"; exit 7 ;;
esac
exec /usr/bin/grep "$@"
"#,
        )?;
        write_executable(
            &bin.join("sort"),
            r#"#!/bin/sh
if [ "${VELNOR_TEST_SORT_FAILURE-}" = yes ]; then exit 8; fi
exec /usr/bin/sort "$@"
"#,
        )?;
        Ok(Self { temp, rustup, bin })
    }

    fn run(
        &self,
        catalog: &ToolCatalog,
        channel: &str,
        overrides: &[(&str, &str)],
    ) -> std::io::Result<Output> {
        let step = super::run_text(catalog).0;
        let mut command = Command::new("sh");
        command
            .args(["-c", &step])
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("RUNNER_TEMP", self.temp.path())
            .env("RUSTUP_HOME", &self.rustup)
            .env("VELNOR_TEST_CHANNEL", channel)
            .env(
                "VELNOR_TEST_URL",
                format!("https://static.rust-lang.org/dist/channel-rust-{RUST_VERSION}.toml"),
            );
        let hash_tool = Command::new("sh")
            .args(["-c", "command -v sha256sum || command -v shasum"])
            .output()?;
        let hash_tool = String::from_utf8_lossy(&hash_tool.stdout).trim().to_owned();
        let hash_kind = if hash_tool.ends_with("shasum") {
            "shasum"
        } else {
            "sha256sum"
        };
        command
            .env("VELNOR_TEST_HASH_TOOL", hash_tool)
            .env("VELNOR_TEST_HASH_KIND", hash_kind);
        if catalog.version(PinnedTool::Rust) != RUST_VERSION {
            command.env("VELNOR_TOOL_RUST", catalog.version(PinnedTool::Rust));
            command.env(
                "VELNOR_TEST_URL",
                format!(
                    "https://static.rust-lang.org/dist/channel-rust-{}.toml",
                    catalog.version(PinnedTool::Rust)
                ),
            );
        }
        for (name, value) in overrides {
            command.env(name, value);
        }
        command.output()
    }
}

fn write_executable(path: &Path, contents: &str) -> std::io::Result<()> {
    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

fn catalog() -> ToolCatalog {
    ToolCatalog::pinned()
}

#[test]
fn downloaded_manifest_digest_mismatch_fails_in_the_executable_step()
-> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog();
    let fixture = ShellFixture::new(&catalog, Some(&format!("{HASH_A}\n")))?;
    let output = fixture.run(&catalog, "not the catalogued manifest", &[])?;
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("rust_channel_manifest_digest_mismatch"),
        "{output:?}"
    );
    Ok(())
}

#[test]
fn installed_hash_absent_from_verified_manifest_fails() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog();
    let fixture = ShellFixture::new(&catalog, Some(&format!("{HASH_B}\n")))?;
    let output = fixture.run(
        &catalog,
        &format!("{HASH_A}\n"),
        &[("VELNOR_TEST_SKIP_DIGEST", "yes")],
    )?;
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("rust_component_hash_not_in_manifest"),
        "{output:?}"
    );
    Ok(())
}

#[test]
fn missing_and_empty_installed_manifests_fail() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog();
    let missing = ShellFixture::new(&catalog, None)?;
    let output = missing.run(
        &catalog,
        &format!("{HASH_A}\n"),
        &[("VELNOR_TEST_SKIP_DIGEST", "yes")],
    )?;
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("rust_installed_manifest_missing"),
        "{output:?}"
    );

    let empty = ShellFixture::new(&catalog, Some(""))?;
    let output = empty.run(
        &catalog,
        &format!("{HASH_A}\n"),
        &[("VELNOR_TEST_SKIP_DIGEST", "yes")],
    )?;
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("rust_installed_manifest_empty"),
        "{output:?}"
    );
    Ok(())
}

#[test]
fn unknown_rust_version_fails_before_fetching_a_manifest() -> Result<(), Box<dyn std::error::Error>>
{
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Rust, "0.0.1")?
        .for_emission();
    let fixture = ShellFixture::new(&catalog, Some(&format!("{HASH_A}\n")))?;
    let output = fixture.run(&catalog, &format!("{HASH_A}\n"), &[])?;
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("rust_channel_manifest_not_catalogued"),
        "{output:?}"
    );
    assert!(!fixture.temp.path().join("curl-called").exists());
    Ok(())
}

#[test]
fn manifest_read_errors_after_partial_grep_output_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog();
    let channel_read = ShellFixture::new(&catalog, Some(&format!("{HASH_A}\n")))?;
    let output = channel_read.run(
        &catalog,
        &format!("{HASH_A}\n"),
        &[
            ("VELNOR_TEST_SKIP_DIGEST", "yes"),
            ("VELNOR_TEST_GREP_FAILURE", "channel"),
        ],
    )?;
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("rust_channel_manifest_read_failed"),
        "{output:?}"
    );

    let installed_read = ShellFixture::new(&catalog, Some(&format!("{HASH_A}\n")))?;
    let output = installed_read.run(
        &catalog,
        &format!("{HASH_A}\n"),
        &[
            ("VELNOR_TEST_SKIP_DIGEST", "yes"),
            ("VELNOR_TEST_GREP_FAILURE", "installed"),
        ],
    )?;
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("rust_installed_manifest_read_failed"),
        "{output:?}"
    );
    Ok(())
}

#[test]
fn valid_download_and_installed_component_hash_pass() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog();
    let fixture = ShellFixture::new(&catalog, Some(&format!("component = \"{HASH_A}\"\n")))?;
    let output = fixture.run(
        &catalog,
        &format!("component = \"{HASH_A}\"\n"),
        &[("VELNOR_TEST_SKIP_DIGEST", "yes")],
    )?;
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("channel manifest matches SHA-256"),
        "The verifier must compare recorded installed-manifest hashes with the channel manifest: {output:?}."
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("It contains every hash recorded in the installed manifest.")
    );
    Ok(())
}

#[test]
fn sorting_pipeline_failure_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog();
    let fixture = ShellFixture::new(&catalog, Some(&format!("{HASH_A}\n")))?;
    let output = fixture.run(
        &catalog,
        &format!("{HASH_A}\n"),
        &[
            ("VELNOR_TEST_SKIP_DIGEST", "yes"),
            ("VELNOR_TEST_SORT_FAILURE", "yes"),
        ],
    )?;
    assert!(!output.status.success(), "{output:?}");
    Ok(())
}
