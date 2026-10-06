//! Real Cargo source-closure regressions; local registry, no network/compiler.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::fetch_script;
use velnor_actions_mise::ToolCatalog;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct RegistryFixture {
    temp: TempDir,
    package: PathBuf,
    registry: PathBuf,
    home: PathBuf,
}

impl RegistryFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let package = temp.path().join("package");
        let registry = temp.path().join("registry");
        let home = temp.path().join("cargo-home");
        fs::create_dir_all(package.join("src"))?;
        fs::create_dir_all(package.join(".cargo"))?;
        fs::create_dir_all(&registry)?;
        fs::create_dir_all(&home)?;
        fs::write(package.join("src/lib.rs"), "")?;
        fs::write(
            package.join("Cargo.toml"),
            "[package]\nname='probe'\nversion='0.1.0'\nedition='2024'\n\
             [dependencies]\nnormal-dep='=1.0.0'\n\
             optional-dep={version='=1.0.0',optional=true}\n\
             [target.'cfg(windows)'.dependencies]\ntarget-dep='=1.0.0'\n",
        )?;
        fs::write(
            package.join(".cargo/config.toml"),
            format!(
                "[source.crates-io]\nreplace-with='fixture'\n\
                 [source.fixture]\nlocal-registry='{}'\n",
                registry.display()
            ),
        )?;
        let fixture = Self {
            temp,
            package,
            registry,
            home,
        };
        for name in ["normal-dep", "optional-dep", "target-dep"] {
            fixture.add_package(name)?;
        }
        assert_success(&fixture.cargo(&["generate-lockfile", "--offline"])?);
        Ok(fixture)
    }

    fn add_package(&self, name: &str) -> TestResult {
        let package_name = format!("{name}-1.0.0");
        let source = self.temp.path().join("archives").join(&package_name);
        fs::create_dir_all(source.join("src"))?;
        fs::write(
            source.join("Cargo.toml"),
            format!("[package]\nname='{name}'\nversion='1.0.0'\nedition='2024'\n"),
        )?;
        fs::write(source.join("src/lib.rs"), "")?;
        let archive = self.archive(name);
        let output = Command::new("tar")
            .env("COPYFILE_DISABLE", "1")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(self.temp.path().join("archives"))
            .arg(&package_name)
            .output()?;
        assert_success(&output);
        let checksum: String = Sha256::digest(fs::read(&archive)?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let index = self
            .registry
            .join("index")
            .join(&name[..2])
            .join(&name[2..4]);
        fs::create_dir_all(&index)?;
        let record = serde_json::json!({
            "name": name, "vers": "1.0.0", "deps": [], "cksum": checksum,
            "features": {}, "yanked": false,
        });
        fs::write(index.join(name), format!("{record}\n"))?;
        Ok(())
    }

    fn archive(&self, name: &str) -> PathBuf {
        self.registry.join(format!("{name}-1.0.0.crate"))
    }

    fn cargo(&self, args: &[&str]) -> std::io::Result<Output> {
        Command::new(env!("CARGO"))
            .args(args)
            .current_dir(&self.package)
            .env("CARGO_HOME", &self.home)
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .output()
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn default_metadata_misses_locked_optional_source() -> TestResult {
    let fixture = RegistryFixture::new()?;
    fs::remove_file(fixture.archive("optional-dep"))?;
    assert_success(&fixture.cargo(&[
        "metadata",
        "--locked",
        "--offline",
        "--format-version",
        "1",
    ])?);
    let probe = fixture.cargo(&["fetch", "--locked", "--offline"])?;
    assert!(!probe.status.success());
    assert!(String::from_utf8_lossy(&probe.stderr).contains("optional-dep"));
    Ok(())
}

#[test]
fn platform_metadata_misses_other_selected_target_source() -> TestResult {
    let fixture = RegistryFixture::new()?;
    fs::remove_file(fixture.archive("target-dep"))?;
    assert_success(&fixture.cargo(&[
        "metadata",
        "--locked",
        "--offline",
        "--format-version",
        "1",
        "--filter-platform",
        "x86_64-unknown-linux-gnu",
    ])?);
    let probe = fixture.cargo(&["fetch", "--locked", "--offline"])?;
    assert!(!probe.status.success());
    assert!(String::from_utf8_lossy(&probe.stderr).contains("target-dep"));
    Ok(())
}

/// The test shim accepts only the pinned isolated Mise invocation.
#[cfg(unix)]
fn write_mise_shim(path: &Path) -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    fs::write(
        path,
        "#!/bin/sh\nset -eu\n\
         test \"$1 $2 $3 $4\" = '--no-config --no-env --no-hooks exec'\n\
         test \"$5\" = \"$FIXTURE_RUST_SPEC\"\nshift 5\n\
         test \"$1\" = '--'\nshift\ntest \"$1\" = 'cargo'\nshift\n\
         test -z \"${GITHUB_TOKEN+x}\"\nexec \"$FIXTURE_CARGO\" \"$@\"\n",
    )?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(unix)]
fn run_generated(fixture: &RegistryFixture) -> Result<Output, Box<dyn std::error::Error>> {
    let bin = fixture.temp.path().join("bin");
    fs::create_dir_all(&bin)?;
    write_mise_shim(&bin.join("mise"))?;
    let catalog = ToolCatalog::pinned();
    let script = velnor_actions_workflow_renderer::toolchain_env::with_credential_unset_script(
        &fetch_script(&catalog, "").expect("fetch script"),
    );
    Ok(Command::new("sh")
        .args(["-c", &script])
        .current_dir(fixture.temp.path())
        .env("GITHUB_WORKSPACE", &fixture.package)
        .env("CARGO_HOME", &fixture.home)
        .env("GITHUB_TOKEN", "fixture-secret")
        .env("FIXTURE_CARGO", env!("CARGO"))
        .env(
            "FIXTURE_RUST_SPEC",
            catalog
                .tool_spec(velnor_actions_mise::PinnedTool::Rust)
                .expect("qualified selector"),
        )
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH")?),
        )
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .output()?)
}

#[cfg(unix)]
#[test]
fn generated_probe_honors_source_replacement_and_skips_complete_warm_fetch() -> TestResult {
    let fixture = RegistryFixture::new()?;
    let output = run_generated(&fixture)?;
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("sources hit, skipping fetch"));
    // The archive was extracted through repo source replacement; clean-cwd
    // probing would instead fail against the empty default registry.
    assert!(fixture.home.join("registry/src").is_dir());
    Ok(())
}

#[cfg(unix)]
#[test]
fn generated_fetch_does_not_hide_missing_optional_source_failure() -> TestResult {
    let fixture = RegistryFixture::new()?;
    fs::remove_file(fixture.archive("optional-dep"))?;
    let output = run_generated(&fixture)?;
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sources miss (source_missing)"));
    assert!(stdout.contains("complete_locked_workspace"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("optional-dep"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
    Ok(())
}
