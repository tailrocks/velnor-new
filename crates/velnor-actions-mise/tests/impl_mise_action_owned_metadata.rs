#![cfg(unix)]

use std::path::{Path, PathBuf};
use velnor_actions_mise::{MetadataDiscovery, MiseError, ToolCatalog, ToolHomes};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> TestResult<Self> {
        let root = crate::test_temp_dir::unique_temp_dir("action-owned-metadata")?;
        std::fs::create_dir(root.join("bin"))?;
        std::fs::create_dir(root.join("manifest with space"))?;
        Ok(Self(root.canonicalize()?))
    }

    fn write_program(&self, name: &str, body: &str) -> TestResult {
        use std::os::unix::fs::PermissionsExt;
        let path = self.0.join("bin").join(name);
        std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("action_owned_metadata_cleanup_failed:{:?}", error.kind());
        }
    }
}

#[test]
fn action_owned_metadata_checks_version_and_shares_tool_homes() -> TestResult {
    if let Some(mode) = std::env::var_os("VELNOR_METADATA_GUARD_CHILD") {
        return run_child(mode.to_string_lossy().as_ref());
    }

    let fixture = Fixture::new()?;
    install_fake_tools(&fixture)?;
    run_cases(&fixture)
}

const FAKE_MISE: &str = r#"
test "$1:$2:$3:$4:$5:$6:$7" = '--no-config:--no-env:--no-hooks:exec:rust@1.99.0:--:mbx'
test "${MISE_AUTO_INSTALL-unset}" = false
phase=version
if [ "$8" != --version ]; then
    test "$#" -eq 14
    test "$8:$9:${10}:${11}:${12}:${13}" = '+1.99.0:metadata:--format-version:1:--no-deps:--manifest-path'
    test "${14}" = "$VELNOR_EXPECTED_MANIFEST"
    phase=metadata
else
    test "$#" -eq 8
fi
if [ "$VELNOR_EXPECT_HOMES" = yes ]; then
    test "$MISE_RUSTUP_HOME" = "$VELNOR_EXPECTED_RUSTUP_HOME"
    test "$MISE_CARGO_HOME" = "$VELNOR_EXPECTED_CARGO_HOME"
    test "$RUSTUP_TOOLCHAIN" = 1.99.0
else
    test "${RUSTUP_TOOLCHAIN-unset}" = unset
fi
printf '%s\n' "$phase" >> "$VELNOR_METADATA_TRACE"
shift 6
exec "$@"
"#;

const FAKE_MBX: &str = r#"
if [ "$1" = --version ]; then
    printf 'mbx %s\n' "$VELNOR_FIXTURE_MBX_VERSION"
    exit 0
fi
test "$1:$2:$3:$4:$5:$6" = '+1.99.0:metadata:--format-version:1:--no-deps:--manifest-path'
test "$7" = "$VELNOR_EXPECTED_MANIFEST"
touch "$VELNOR_METADATA_MARKER"
printf '{"packages":[]}\n'
"#;

fn install_fake_tools(fixture: &Fixture) -> TestResult {
    fixture.write_program("mise", FAKE_MISE)?;
    fixture.write_program("mbx", FAKE_MBX)
}

fn run_cases(fixture: &Fixture) -> TestResult {
    for (mode, version, expected_trace, metadata_runs) in [
        ("wrong-command", "1.22.0", "version\n", false),
        ("wrong-request", "1.22.0", "version\n", false),
        ("invalid-rustup-selector", "1.23.0", "", false),
        ("valid-command", "1.23.0", "version\nmetadata\n", true),
    ] {
        run_case(fixture, mode, version, expected_trace, metadata_runs)?;
    }
    Ok(())
}

fn run_case(
    fixture: &Fixture,
    mode: &str,
    version: &str,
    expected_trace: &str,
    metadata_runs: bool,
) -> TestResult {
    let trace = fixture.0.join(format!("{mode}.trace"));
    let marker = fixture.0.join(format!("{mode}.metadata-ran"));
    let manifest = fixture.0.join("manifest with space/Cargo.toml");
    let output = child_process(fixture, mode, version, &manifest, &trace, &marker)?.output()?;
    assert!(
        output.status.success(),
        "{mode} child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual_trace = if trace.exists() {
        std::fs::read_to_string(trace)?
    } else {
        String::new()
    };
    assert_eq!(actual_trace, expected_trace, "{mode}");
    assert_eq!(marker.exists(), metadata_runs, "{mode}");
    Ok(())
}

fn child_process(
    fixture: &Fixture,
    mode: &str,
    version: &str,
    manifest: &Path,
    trace: &Path,
    marker: &Path,
) -> TestResult<std::process::Command> {
    let binary = std::env::current_exe()?;
    let expected_homes = if mode == "wrong-request" { "no" } else { "yes" };
    let bin = fixture.0.join("bin");
    let mut command = std::process::Command::new(binary);
    command
        .args([
            "--exact",
            "impl_mise_action_owned_metadata::action_owned_metadata_checks_version_and_shares_tool_homes",
            "--nocapture",
        ])
        .env("VELNOR_METADATA_GUARD_CHILD", mode)
        .env("VELNOR_FIXTURE_MBX_VERSION", version)
        .env("VELNOR_METADATA_TRACE", trace)
        .env("VELNOR_METADATA_MARKER", marker)
        .env("VELNOR_EXPECTED_MANIFEST", manifest)
        .env("VELNOR_EXPECT_HOMES", expected_homes)
        .env("VELNOR_EXPECTED_RUSTUP_HOME", fixture.0.join("owned-rustup"))
        .env("VELNOR_EXPECTED_CARGO_HOME", fixture.0.join("owned-cargo"))
        .env("MISE_RUSTUP_HOME", fixture.0.join("ambient-rustup"))
        .env("MISE_CARGO_HOME", fixture.0.join("ambient-cargo"))
        .env("RUSTUP_TOOLCHAIN", "ambient-toolchain")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    Ok(command)
}

fn run_child(mode: &str) -> TestResult {
    let manifest = PathBuf::from(std::env::var_os("VELNOR_EXPECTED_MANIFEST").ok_or("manifest")?);
    let request = MetadataDiscovery::new(manifest)?;
    let catalog = ToolCatalog::pinned();
    let result = match mode {
        "wrong-request" => request.run(&catalog).map(|_| ()),
        "invalid-rustup-selector" => {
            let result = request
                .command(&catalog)?
                .with_env(&[("RUSTUP_TOOLCHAIN".into(), "1.98.0".into())]);
            assert!(matches!(
                result,
                Err(MiseError::InvalidStepInput {
                    ref field,
                    ref value,
                }) if field == "RUSTUP_TOOLCHAIN" && value == "must_match_catalog_rust_pin"
            ));
            Ok(())
        }
        "wrong-command" | "valid-command" => {
            let rustup =
                Path::new(&std::env::var_os("VELNOR_EXPECTED_RUSTUP_HOME").ok_or("rustup")?)
                    .to_owned();
            let cargo = Path::new(&std::env::var_os("VELNOR_EXPECTED_CARGO_HOME").ok_or("cargo")?)
                .to_owned();
            let rustup = rustup.to_string_lossy();
            let cargo = cargo.to_string_lossy();
            let homes = ToolHomes::new(&rustup, &cargo)?;
            let command = request.command(&catalog)?.with_env(&homes.env(&catalog))?;
            assert_eq!(command.argv(), request.argv(&catalog));
            assert!(command.disables_auto_install());
            command.run().and_then(|output| {
                if mode == "valid-command" && output.success {
                    assert_eq!(output.stdout, b"{\"packages\":[]}\n");
                    Ok(())
                } else {
                    Err(MiseError::EmptyCommand {
                        program: "unexpected_metadata_result".to_owned(),
                    })
                }
            })
        }
        _ => return Err("unknown child mode".into()),
    };
    if mode.starts_with("wrong-") {
        assert!(matches!(
            result,
            Err(MiseError::InvalidToolVersion { ref tool, ref version })
                if tool == "mr-boxington" && version == "1.22.0"
        ));
    } else {
        result?;
    }
    Ok(())
}
