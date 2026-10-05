use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    os::unix::fs::symlink,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

use velnor_actions_contract::StepKind;

use crate::setup::MiseSetup;

use super::ToolsCachePayload;

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    runner_temp: PathBuf,
    github_output: PathBuf,
}

impl Fixture {
    fn new() -> std::io::Result<Self> {
        let base = fs::canonicalize(std::env::current_dir()?)?;
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = base.join(format!(
            ".tool-cache-identity-{}-{id} space",
            std::process::id()
        ));
        let home = root.join("home user");
        let runner_temp = root.join("runner temp");
        let github_output = root.join("github output");
        fs::create_dir_all(home.join(".local/share"))?;
        fs::create_dir_all(&runner_temp)?;
        fs::write(&github_output, "")?;
        Ok(Self {
            root,
            home,
            runner_temp,
            github_output,
        })
    }

    fn output(&self) -> std::io::Result<String> {
        fs::read_to_string(&self.github_output)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.exists() {
            match fs::remove_dir_all(&self.root) {
                Ok(()) => {}
                Err(error) => eprintln!("fixture cleanup failed: {error}"),
            }
        }
    }
}

fn payload(runs_on: &str) -> ToolsCachePayload {
    let setup = MiseSetup {
        uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "a".repeat(64),
    };
    ToolsCachePayload::new(super::super::ToolsCacheInputs {
        runs_on,
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &setup,
        tool_specs: &["rust@1.98.1".to_owned()],
        rustup_toolchain: Some("1.98.1"),
        rustup_components: &[],
    })
    .expect("typed payload")
}

fn run_identity(
    payload: &ToolsCachePayload,
    fixture: &Fixture,
    overrides: &[(&str, String)],
) -> Output {
    run_identity_with(payload, fixture, overrides, &[])
}

fn run_identity_with(
    payload: &ToolsCachePayload,
    fixture: &Fixture,
    overrides: &[(&str, String)],
    removed: &[&str],
) -> Output {
    identity_command(payload, fixture, overrides, removed)
        .output()
        .expect("run identity shell")
}

fn identity_command(
    payload: &ToolsCachePayload,
    fixture: &Fixture,
    overrides: &[(&str, String)],
    removed: &[&str],
) -> Command {
    fs::write(&fixture.github_output, "").expect("clear outputs");
    if payload.runtime_identity_supported() {
        let step = payload.runtime_identity_step().expect("identity action");
        let StepKind::Action { uses, with, env } = step.kind else {
            panic!("identity is a composite action step");
        };
        assert_eq!(Some(uses.as_str()), super::action_uses(&payload.runs_on));
        assert!(env.is_empty());
        assert_eq!(
            with.get("d").map(String::as_str),
            Some(payload.static_digest())
        );
    }
    let inner = super::inner_step(&payload.runs_on).expect("composite identity shell step");
    let StepKind::Shell { run, env } = inner.kind else {
        panic!("composite identity body is a shell step");
    };
    let script_file = super::script_file(env!("CARGO_PKG_VERSION")).expect("identity script file");
    let script_path = fixture.root.join(&script_file.path);
    fs::create_dir_all(script_path.parent().expect("script parent"))
        .expect("create script directory");
    fs::write(&script_path, script_file.bytes).expect("write generated script");
    let action_path = super::action_uses(&payload.runs_on).map_or_else(
        || fixture.root.join(".github/actions/test"),
        |uses| fixture.root.join(uses.trim_start_matches("./")),
    );
    fs::create_dir_all(&action_path).expect("create action directory");
    let mut command = Command::new("env");
    command
        .args(run)
        .current_dir(&fixture.root)
        .env_clear()
        .envs(env)
        .env("HOME", &fixture.home)
        .env("RUNNER_TEMP", &fixture.runner_temp)
        .env(
            "MISE_RUSTUP_HOME",
            fixture.runner_temp.join("velnor/rustup"),
        )
        .env("RUSTUP_HOME", fixture.runner_temp.join("velnor/rustup"))
        .env("MISE_CARGO_HOME", fixture.runner_temp.join("velnor/cargo"))
        .env("CARGO_HOME", fixture.runner_temp.join("velnor/cargo"))
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("ImageOS", "ubuntu26")
        .env("ImageVersion", "20261004.1")
        .env("GITHUB_ACTION_PATH", &action_path)
        .env("VELNOR_CACHE_LANE", &payload.runs_on)
        .env("VELNOR_CACHE_STATIC_DIGEST", payload.static_digest())
        .env("GITHUB_OUTPUT", &fixture.github_output);
    for (key, value) in overrides {
        command.env(key, value);
    }
    for key in removed {
        command.env_remove(key);
    }
    command
}

fn assert_disabled(output: &Output, text: &str, reason: &str) {
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        text.lines().find(|line| line.starts_with("enabled=")),
        Some("enabled=false")
    );
    assert!(
        text.lines().any(|line| line == format!("reason={reason}")),
        "expected reason {reason}, got {text:?}"
    );
    assert!(!text.lines().any(|line| line.starts_with("identity=")));
}

fn identity(text: &str) -> &str {
    text.lines()
        .find_map(|line| line.strip_prefix("identity="))
        .expect("usable runtime identity")
}

fn run_and_read(
    payload: &ToolsCachePayload,
    fixture: &Fixture,
    overrides: &[(&str, String)],
) -> (Output, String) {
    let output = run_identity(payload, fixture, overrides);
    let text = fixture.output().expect("read outputs");
    (output, text)
}

fn run_and_read_removed(
    payload: &ToolsCachePayload,
    fixture: &Fixture,
    removed: &[&str],
) -> (Output, String) {
    let output = run_identity_with(payload, fixture, &[], removed);
    let text = fixture.output().expect("read outputs");
    (output, text)
}

#[test]
fn hosted_identity_hashes_the_validated_image_and_owned_roots() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let (output, text) = run_and_read(&payload("ubuntu-26.04"), &fixture, &[]);
    assert!(output.status.success(), "{output:?}");
    assert!(text.lines().any(|line| line == "enabled=true"), "{text}");
    let fingerprint = identity(&text);
    assert_eq!(fingerprint.len(), 64);
    assert!(fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(
        text.lines()
            .any(|line| line == "reason=qualified_hosted_image")
    );
    Ok(())
}

#[test]
fn generated_identity_actions_bind_each_supported_hosted_lane() -> Result<(), Box<dyn Error>> {
    for (lane, path, uses) in [
        (
            "ubuntu-22.04",
            ".github/actions/u22/action.yml",
            "./.github/actions/u22",
        ),
        (
            "ubuntu-24.04",
            ".github/actions/u24/action.yml",
            "./.github/actions/u24",
        ),
        (
            "ubuntu-26.04",
            ".github/actions/u26/action.yml",
            "./.github/actions/u26",
        ),
    ] {
        let file = super::action_file(lane, env!("CARGO_PKG_VERSION"))?;
        assert_eq!(file.path, path);
        assert!(file.bytes.contains(&format!("VELNOR_CACHE_LANE: {lane}")));
        assert_eq!(super::action_uses(lane), Some(uses));
    }
    Ok(())
}

#[test]
fn hosted_identity_disables_on_image_arch_or_version_mismatch() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let hosted = payload("ubuntu-26.04");
    let cases = [
        ("ImageOS", "ubuntu24", "image_os_mismatch"),
        ("RUNNER_ARCH", "ARM64", "runner_arch_mismatch"),
        ("ImageVersion", "", "image_version_missing"),
    ];
    for (key, value, reason) in cases {
        let (output, text) = run_and_read(&hosted, &fixture, &[(key, value.to_owned())]);
        assert_disabled(&output, &text, reason);
    }
    for (variable, reason) in [
        ("RUNNER_OS", "runner_os_missing"),
        ("RUNNER_ARCH", "runner_arch_missing"),
        ("ImageOS", "image_os_missing"),
        ("ImageVersion", "image_version_missing"),
    ] {
        let (output, text) = run_and_read_removed(&hosted, &fixture, &[variable]);
        assert_disabled(&output, &text, reason);
    }
    let (output, text) = run_and_read(&payload("ubuntu-26.04-arm"), &fixture, &[]);
    assert_disabled(&output, &text, "lane_image_unqualified");
    Ok(())
}

#[test]
fn hosted_identity_disables_for_unarchived_or_aliased_mise_roots() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let payload = payload("ubuntu-26.04");
    let alternative = fixture.root.join("mise elsewhere");
    fs::create_dir_all(&alternative)?;
    let (output, text) = run_and_read(
        &payload,
        &fixture,
        &[("MISE_DATA_DIR", alternative.to_string_lossy().into_owned())],
    );
    assert_disabled(&output, &text, "mise_root_not_archived");

    let xdg = fixture.root.join("xdg data");
    fs::create_dir_all(&xdg)?;
    let (output, text) = run_and_read(
        &payload,
        &fixture,
        &[("XDG_DATA_HOME", xdg.to_string_lossy().into_owned())],
    );
    assert_disabled(&output, &text, "mise_root_not_archived");

    let actual = fixture.home.join("mise-store");
    fs::create_dir_all(&actual)?;
    let mise = fixture.home.join(".local/share/mise");
    symlink(&actual, &mise)?;
    let (output, text) = run_and_read(&payload, &fixture, &[]);
    assert_disabled(&output, &text, "mise_root_aliased");
    Ok(())
}

#[test]
fn hosted_identity_disables_for_invalid_native_or_mise_homes() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let payload = payload("ubuntu-26.04");
    let mismatch = fixture.root.join("other cargo");
    let (output, text) = run_and_read(
        &payload,
        &fixture,
        &[("CARGO_HOME", mismatch.to_string_lossy().into_owned())],
    );
    assert_disabled(&output, &text, "cargo_home_mismatch");

    let (output, text) = run_and_read(
        &payload,
        &fixture,
        &[("RUSTUP_HOME", "relative/rustup".to_owned())],
    );
    assert_disabled(&output, &text, "rustup_home_mismatch");

    let (output, text) = run_and_read_removed(&payload, &fixture, &["MISE_RUSTUP_HOME"]);
    assert_disabled(&output, &text, "mise_rustup_home_mismatch");
    Ok(())
}

#[test]
fn hosted_identity_disables_for_root_home_or_runner_temp() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let hosted = payload("ubuntu-26.04");
    for (key, reason) in [
        ("HOME", "home_aliased"),
        ("RUNNER_TEMP", "runner_temp_aliased"),
    ] {
        let (output, text) = run_and_read(&hosted, &fixture, &[(key, "/".to_owned())]);
        assert_disabled(&output, &text, reason);
    }
    Ok(())
}

#[test]
fn scale_set_identity_is_a_successful_cold_path_without_digest_injection()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let scale_set = payload("scale-set:velnor+ubuntu-26.04-scale-set");
    let (output, text) = run_and_read(&scale_set, &fixture, &[]);
    assert_disabled(&output, &text, "scale_set_image_unqualified");
    Ok(())
}

#[test]
fn identity_step_binds_fixed_tool_home_environment() {
    let payload = payload("ubuntu-26.04");
    let step = payload.runtime_identity_step().expect("identity step");
    let StepKind::Action { uses, with, env } = step.kind else {
        panic!("identity is a composite action step");
    };
    assert_eq!(Some(uses.as_str()), super::action_uses("ubuntu-26.04"));
    assert!(env.is_empty());
    assert_eq!(
        with.get("d").map(String::as_str),
        Some(payload.static_digest())
    );
    let inner = super::inner_step("ubuntu-26.04").expect("composite identity shell step");
    let StepKind::Shell { env, .. } = inner.kind else {
        panic!("composite identity body is a shell step");
    };
    let homes = BTreeMap::from([
        ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
        ("CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
    ]);
    for (key, value) in homes {
        assert_eq!(env.get(key).map(String::as_str), Some(value));
    }
    assert_eq!(step.name, crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME);
}

#[path = "cache_p08_runtime_identity_fingerprint_tests.rs"]
mod fingerprint_tests;

#[path = "cache_p08_runtime_identity_scratch_tests.rs"]
mod scratch_tests;
