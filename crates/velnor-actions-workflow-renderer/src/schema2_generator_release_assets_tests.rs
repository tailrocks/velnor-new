use super::{
    LINUX, MACOS_ARM64, MACOS_X86_64, ProductAsset, build_steps, qualification_script,
    verify_provenance_in_directory,
};

use crate::schema2::git_fixture;
use crate::yaml::Yaml;

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use velnor_actions_contract::ReleaseTarget;

const TOKEN_SENTINEL: &str = "nonsecret-token-fixture";
const TOKEN_EXPRESSION: &str = "${{ github.token }}";
const INSTALL_ARGS: &[&str] = &[
    "--no-config",
    "--no-env",
    "--no-hooks",
    "install",
    "rust@1.98.1",
    "mr-boxington@1.21.1",
];

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-release-mise-token-{}-{nonce}",
            std::process::id()
        ));
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
fn every_target_install_receives_token_and_build_scrubs_it() -> Result<(), Box<dyn Error>> {
    let pins = release_test_pins();
    for (product, host) in [
        (LINUX, ReleaseTarget::LinuxX86_64),
        (MACOS_ARM64, ReleaseTarget::MacosArm64),
        (MACOS_X86_64, ReleaseTarget::MacosArm64),
    ] {
        let steps = build_steps(product, host, "Verify artifact", "true", &pins)?;
        let install = step_named(&steps, "Install pinned Rust and MBX")?;
        assert_workflow_token_mapping(install)?;
        assert_install_invocation(install)?;
        let build = step_named(&steps, "Build velnor-actions with MBX")?;
        assert_build_invocation_scrubs_token(build, product)?;
    }
    Ok(())
}

fn release_test_pins() -> crate::schema2::ProductReleasePins {
    let mut pins = crate::schema2::product_release_test_pins::test_pins();
    pins.install_build_tools_argv = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "install",
        "rust@1.98.1",
        "mr-boxington@1.21.1",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    pins
}

fn step_named<'a>(steps: &'a [Yaml], name: &str) -> Result<&'a Yaml, Box<dyn Error>> {
    steps
        .iter()
        .find(|step| field(step, "name").is_ok_and(|value| value == &Yaml::str(name)))
        .ok_or_else(|| format!("missing rendered step {name}").into())
}

fn field<'a>(step: &'a Yaml, key: &str) -> Result<&'a Yaml, Box<dyn Error>> {
    let Yaml::Map(fields) = step else {
        return Err("rendered step must be a mapping".into());
    };
    fields
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .ok_or_else(|| format!("rendered step lacks {key}").into())
}

fn command(step: &Yaml) -> Result<&str, Box<dyn Error>> {
    let Yaml::Str(run) = field(step, "run")? else {
        return Err("rendered command must be a string".into());
    };
    Ok(run)
}

fn assert_workflow_token_mapping(step: &Yaml) -> Result<(), Box<dyn Error>> {
    let expected = Yaml::Map(vec![(
        "GITHUB_TOKEN".to_owned(),
        Yaml::str(TOKEN_EXPRESSION),
    )]);
    assert_eq!(field(step, "env")?, &expected);
    assert!(command(step)?.starts_with("set -eu\n: \"${GITHUB_TOKEN:?missing workflow token}\"\n"));
    Ok(())
}

fn assert_install_invocation(step: &Yaml) -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let mock_bin = scratch.0.join("mock-bin");
    fs::create_dir(&mock_bin)?;
    let token_marker = scratch.0.join("token-presence");
    let argv_marker = scratch.0.join("mise-argv");
    write_executable(
        &mock_bin.join("mise"),
        "#!/bin/sh\n[ \"${GITHUB_TOKEN:-}\" = 'nonsecret-token-fixture' ] || exit 91\nprintf 'present\\n' > \"$TOKEN_MARKER\"\nprintf '%s\\n' \"$@\" > \"$ARGV_MARKER\"\n",
    )?;
    let mut install = run_step(command(step)?, &scratch.0, &mock_bin, TOKEN_SENTINEL)?;
    install
        .env("TOKEN_MARKER", &token_marker)
        .env("ARGV_MARKER", &argv_marker);
    let output = install.output()?;
    assert!(
        output.status.success(),
        "Mise install fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(token_marker)?, "present\n");
    assert_eq!(fs::read_to_string(argv_marker)?, expected_install_argv());
    assert_no_sentinel_output(&output);
    Ok(())
}

fn expected_install_argv() -> String {
    INSTALL_ARGS.join("\n") + "\n"
}

fn assert_build_invocation_scrubs_token(
    step: &Yaml,
    product: ProductAsset,
) -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let mock_bin = scratch.0.join("mock-bin");
    fs::create_dir(&mock_bin)?;
    let token_state = scratch.0.join("token-state");
    let source = if product.target == ReleaseTarget::MacosX86_64 {
        "target/x86_64-apple-darwin/release/velnor-actions"
    } else {
        "target/release/velnor-actions"
    };
    write_executable(
        &mock_bin.join("mise"),
        "#!/bin/sh\nif [ \"${GITHUB_TOKEN+x}\" = x ]; then printf 'present\\n' > \"$TOKEN_STATE\"; exit 92; fi\nmkdir -p \"$(dirname \"$ARTIFACT_SOURCE\")\"\nprintf 'artifact\\n' > \"$ARTIFACT_SOURCE\"\nprintf 'absent\\n' > \"$TOKEN_STATE\"\n",
    )?;
    let mut build = run_step(command(step)?, &scratch.0, &mock_bin, TOKEN_SENTINEL)?;
    build
        .env("TOKEN_STATE", &token_state)
        .env("ARTIFACT_SOURCE", source);
    let output = build.output()?;
    assert!(
        output.status.success(),
        "build fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(token_state)?, "absent\n");
    assert!(scratch.0.join(product.binary).is_file());
    assert_no_sentinel_output(&output);
    Ok(())
}

fn run_step<'a>(
    script: &'a str,
    current_dir: &'a std::path::Path,
    mock_bin: &'a std::path::Path,
    token: &'a str,
) -> Result<Command, Box<dyn Error>> {
    let mut paths = vec![mock_bin.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").ok_or("missing PATH")?,
    ));
    let mut command = Command::new("bash");
    command
        .args(["-c", script])
        .current_dir(current_dir)
        .env("PATH", std::env::join_paths(paths)?)
        .env("GITHUB_TOKEN", token);
    Ok(command)
}

fn assert_no_sentinel_output(output: &std::process::Output) {
    assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN_SENTINEL));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TOKEN_SENTINEL));
}

fn write_executable(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[test]
fn wrong_checksum_filename_stops_before_candidate_execution() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory_name = format!("target/release-sidecar-test-{}-{nonce}", std::process::id());
    let directory = root.join(&directory_name);
    fs::create_dir_all(&directory)?;
    let scratch = Scratch(directory.clone());
    let executed = scratch.0.join("candidate-executed");
    let candidate = directory.join(LINUX.binary);
    fs::write(
        &candidate,
        "#!/bin/sh\nprintf '%s\\n' executed >> \"$CANDIDATE_EXECUTED\"\nprintf '%s\\n' 'velnor-actions 0.1.5'\n",
    )?;
    let mut permissions = fs::metadata(&candidate)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&candidate, permissions)?;
    fs::write(
        directory.join(LINUX.sidecar),
        format!("{}  unrelated-binary\n", "a".repeat(64)),
    )?;
    let sha = git_fixture::command(&root)?
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !sha.status.success() {
        return Err("cannot read candidate source SHA".into());
    }
    let source_sha = String::from_utf8(sha.stdout)?.trim().to_owned();
    let command = format!(
        "{}\n{}",
        verify_provenance_in_directory(LINUX, &directory_name, "1.98.1", "1.21.1"),
        qualification_script(LINUX.binary, &directory_name)
    );
    let status = Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(&root)
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_SHA", source_sha)
        .env("GITHUB_WORKSPACE", &root)
        .env("CANDIDATE_EXECUTED", &executed)
        .status()?;
    assert!(!status.success(), "accepted a sidecar for another filename");
    assert!(
        !executed.exists(),
        "candidate ran before its sidecar passed validation"
    );
    Ok(())
}
