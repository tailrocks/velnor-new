#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::ffi::OsString;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use velnor_actions_mise::command::{IsolatedCommand, MISE_GLOBAL_FLAGS};

#[path = "impl_miserc_isolation_fixture.rs"]
mod fixture;
use fixture::{Layer, Stage};

const FIXED_RELEASE: &str = "2026.10.2";
const FIXED_SOURCE: &str = "44ea2537166efbe21b19d808355d9914e830941a";
const FIXED_LINUX_X64_DIGEST: &str =
    "8f5f6660336f572830e33cd9b378d3131e529a0d4c4f0c553776be90a1ba302a";
const FIXED_BINARY_ENV: &str = "VELNOR_MISE_FIXED_BINARY";
const AFFECTED_RELEASE: &str = "2026.10.1";
const AFFECTED_SOURCE: &str = "050ce5a20287a0aafd872b1191699a5fdafff5ac";
const AFFECTED_LINUX_X64_DIGEST: &str =
    "31e6859cf639ed4594906da3fcd0fe2055e9daddae75e9786dbe50b3fb3c0f4a";
const AFFECTED_BINARY_ENV: &str = "VELNOR_MISE_AFFECTED_BINARY";
const PRODUCT_STAGE_ENV: &str = "VELNOR_MISERC_PRODUCT_STAGE";
const PRODUCT_STAGE_TOKEN_ENV: &str = "VELNOR_MISERC_PRODUCT_STAGE_TOKEN";

#[derive(Clone, Copy)]
enum Selector {
    Flag,
    Environment,
    None,
}

#[derive(Clone, Copy)]
enum Operation {
    Version,
    Exec,
}

fn expected_digest(release: &str) -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH, release) {
        ("linux", "x86_64", FIXED_RELEASE) => Ok(FIXED_LINUX_X64_DIGEST),
        ("linux", "x86_64", AFFECTED_RELEASE) => Ok(AFFECTED_LINUX_X64_DIGEST),
        (os, arch, _) => Err(format!("official digest unavailable for {os}/{arch}")),
    }
}

fn executable_digest(path: &Path) -> Result<String, String> {
    let (tool, output) = if cfg!(target_os = "macos") {
        (
            "/usr/bin/shasum",
            Command::new("/usr/bin/shasum")
                .args(["-a", "256"])
                .arg(path)
                .env_clear()
                .output(),
        )
    } else {
        (
            "/usr/bin/sha256sum",
            Command::new("/usr/bin/sha256sum")
                .arg(path)
                .env_clear()
                .output(),
        )
    };
    let output = output.map_err(|err| format!("cannot hash {}: {err}", path.display()))?;
    if !output.status.success() {
        return Err(format!("{tool} failed to hash {}", path.display()));
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| format!("{tool} returned no digest for {}", path.display()))
}

fn official_binary(env_key: &str, release: &str, source: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os(env_key)
        .map(PathBuf::from)
        .ok_or_else(|| format!("required official mise binary is unset: {env_key}"))?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|err| err.to_string())?;
    if !path.is_absolute() || !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "{env_key} must be an absolute regular file path: {}",
            path.display()
        ));
    }
    let path = path.canonicalize().map_err(|err| err.to_string())?;
    let actual_digest = executable_digest(&path)?;
    let expected = expected_digest(release)?;
    if actual_digest != expected {
        return Err(format!(
            "{env_key} digest mismatch: expected {expected}, got {actual_digest}"
        ));
    }
    let stage = Stage::new()?;
    let mut command = Command::new(&path);
    stage.env(&mut command);
    let output = command
        .arg("version")
        .current_dir(&stage.root)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run {}: {err}", path.display()))?;
    let text = output_text(&output);
    if !output.status.success() || !text.contains(release) {
        return Err(format!(
            "{env_key} must be official mise {release} (source {source}); got {text}"
        ));
    }
    eprintln!(
        "mise qualification binary: release={release} source={source} sha256={actual_digest} path={}",
        path.display(),
    );
    Ok(path)
}

fn invoke(
    binary: &Path,
    stage: &Stage,
    cwd: &Path,
    selector: Selector,
    operation: Operation,
) -> Result<Output, String> {
    let mut command = Command::new(binary);
    stage.env(&mut command);
    match selector {
        Selector::Flag => {
            command.arg("--no-config");
        }
        Selector::Environment => {
            command.env("MISE_NO_CONFIG", "1");
        }
        Selector::None => {}
    }
    match operation {
        Operation::Version => {
            command.arg("version");
        }
        Operation::Exec => {
            command.args(["exec", "--", "/bin/true"]);
        }
    }
    command
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run {}: {err}", binary.display()))
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_reached_bad_file(output: &Output, displayed_path: &str) {
    let text = output_text(output);
    assert!(
        !output.status.success(),
        "malformed config unexpectedly passed"
    );
    assert!(
        text.contains("TOML"),
        "expected TOML parse error, got {text}"
    );
    assert!(
        text.contains(displayed_path),
        "failure did not identify {displayed_path}: {text}"
    );
}

#[test]
#[ignore = "official mise releases are selected by the mandatory real-binary qualification lane"]
fn fixed_release_flag_and_environment_matrix_skips_all_miserc_layers() -> Result<(), String> {
    let binary = official_binary(FIXED_BINARY_ENV, FIXED_RELEASE, FIXED_SOURCE)?;
    let stage = Stage::new()?;
    stage.write_bad_layers(&[Layer::Project, Layer::Global, Layer::System])?;
    for selector in [Selector::Flag, Selector::Environment] {
        for operation in [Operation::Version, Operation::Exec] {
            for cwd in [&stage.root, &stage.nested] {
                let output = invoke(&binary, &stage, cwd, selector, operation)?;
                assert!(
                    output.status.success(),
                    "mise {FIXED_RELEASE} must skip every malformed layer at {}: {}",
                    cwd.display(),
                    output_text(&output)
                );
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "official mise releases are selected by the mandatory real-binary qualification lane"]
fn affected_release_controls_fail_for_both_no_config_selectors() -> Result<(), String> {
    let binary = official_binary(AFFECTED_BINARY_ENV, AFFECTED_RELEASE, AFFECTED_SOURCE)?;
    let stage = Stage::new()?;
    stage.write_bad_layers(&[Layer::Project])?;
    let bad_project = stage.root.join(".miserc.toml");
    for selector in [Selector::Flag, Selector::Environment] {
        let output = invoke(&binary, &stage, &stage.root, selector, Operation::Version)?;
        assert_reached_bad_file(&output, &bad_project.display().to_string());
    }
    Ok(())
}

#[test]
#[ignore = "official mise releases are selected by the mandatory real-binary qualification lane"]
fn no_selector_controls_reach_each_malformed_config_layer() -> Result<(), String> {
    let binary = official_binary(FIXED_BINARY_ENV, FIXED_RELEASE, FIXED_SOURCE)?;
    for (layer, path_of) in [
        (Layer::Project, 0_u8),
        (Layer::Global, 1_u8),
        (Layer::System, 2_u8),
    ] {
        let stage = Stage::new()?;
        stage.write_bad_layers(&[layer])?;
        let displayed_path = match path_of {
            0 => stage.root.join(".miserc.toml").display().to_string(),
            1 => "~/.config/mise/miserc.toml".to_owned(),
            _ => stage.system.join("miserc.toml").display().to_string(),
        };
        let output = invoke(
            &binary,
            &stage,
            &stage.root,
            Selector::None,
            Operation::Version,
        )?;
        assert_reached_bad_file(&output, &displayed_path);
    }
    Ok(())
}

#[path = "impl_miserc_isolation_product.rs"]
mod product;
