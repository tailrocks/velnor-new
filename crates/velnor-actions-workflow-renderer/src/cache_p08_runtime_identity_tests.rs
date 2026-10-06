use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

use velnor_actions_contract::StepKind;

use crate::setup::MiseSetup;

use crate::cache_p08::ToolsCachePayload;

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
    payload_with_specs(runs_on, &["rust@1.98.1".to_owned()])
}

fn payload_with_specs(runs_on: &str, tool_specs: &[String]) -> ToolsCachePayload {
    let setup = MiseSetup {
        uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "a".repeat(64),
    };
    ToolsCachePayload::new(super::super::ToolsCacheInputs {
        runs_on,
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &setup,
        tool_specs,
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

fn emitted_identity_environment(payload: &ToolsCachePayload) -> BTreeMap<String, String> {
    let inner = super::inner_step(&payload.runs_on).expect("composite identity shell step");
    let StepKind::Shell { env: shell_env, .. } = inner.kind else {
        panic!("composite identity body is a shell step");
    };
    if !payload.runtime_identity_supported() {
        let mut shell_env = shell_env;
        shell_env.remove("VELNOR_CACHE_STATIC_DIGEST");
        return shell_env;
    }

    let step = payload.runtime_prelude_step().expect("identity prelude");
    let StepKind::Action {
        uses,
        with,
        env: action_env,
    } = step.kind
    else {
        panic!("identity is a composite action step");
    };
    assert_eq!(
        Some(uses.as_str()),
        crate::cache_p08::runtime_prelude_action_uses(&payload.runs_on)
    );
    assert!(action_env.is_empty());
    let input = crate::cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
    let input_value = with.get(input).expect("emitted digest input");
    assert_eq!(input_value, payload.static_digest());

    let file = super::action_file(&payload.runs_on, env!("CARGO_PKG_VERSION"))
        .expect("generated identity action");
    assert!(file.bytes.contains(&format!("  {input}:\n")));
    let expected_expression = format!("${{{{ inputs.{input} }}}}");
    assert!(file.bytes.contains(&format!(
        "VELNOR_CACHE_STATIC_DIGEST: {expected_expression}"
    )));
    assert_eq!(
        shell_env.get("VELNOR_CACHE_STATIC_DIGEST"),
        Some(&expected_expression)
    );

    let mut resolved = shell_env;
    resolved.insert("VELNOR_CACHE_STATIC_DIGEST".to_owned(), input_value.clone());
    resolved
}

fn identity_command(
    payload: &ToolsCachePayload,
    fixture: &Fixture,
    overrides: &[(&str, String)],
    removed: &[&str],
) -> Command {
    fs::write(&fixture.github_output, "").expect("clear outputs");
    let inner = super::inner_step(&payload.runs_on).expect("composite identity shell step");
    let StepKind::Shell { run, .. } = inner.kind else {
        panic!("composite identity body is a shell step");
    };
    let env = emitted_identity_environment(payload);
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
        "expected {reason} in identity output: {text}"
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
fn static_tools_digest_is_bound_into_the_canonical_runtime_key() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let base = payload("ubuntu-26.04");
    let expanded = payload_with_specs(
        "ubuntu-26.04",
        &["rust@1.98.1".to_owned(), "shellcheck@0.11.0".to_owned()],
    );
    assert_ne!(base.static_digest(), expanded.static_digest());
    assert_eq!(
        base.key_expression(),
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION
    );
    assert!(!base.key_expression().contains(base.static_digest()));

    let (base_output, base_text) = run_and_read(&base, &fixture, &[]);
    let (expanded_output, expanded_text) = run_and_read(&expanded, &fixture, &[]);
    assert!(base_output.status.success(), "{base_output:?}");
    assert!(expanded_output.status.success(), "{expanded_output:?}");
    let base_identity = identity(&base_text);
    let expanded_identity = identity(&expanded_text);
    assert_ne!(base_identity, expanded_identity);
    assert_ne!(
        base.key_for_runtime_identity(base_identity)?,
        expanded.key_for_runtime_identity(expanded_identity)?
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
        let input = crate::cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
        assert!(file.bytes.contains(&format!("  {input}:\n")));
        assert!(file.bytes.contains(&format!(
            "VELNOR_CACHE_STATIC_DIGEST: ${{{{ inputs.{input} }}}}"
        )));
        assert_eq!(super::action_uses(lane), Some(uses));
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
    let step = payload.runtime_prelude_step().expect("identity prelude");
    let StepKind::Action { uses, with, env } = step.kind else {
        panic!("identity is a composite action step");
    };
    assert_eq!(
        Some(uses.as_str()),
        crate::cache_p08::runtime_prelude_action_uses("ubuntu-26.04")
    );
    assert!(env.is_empty());
    assert_eq!(
        with.get(crate::cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
            .map(String::as_str),
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

#[path = "cache_p08_runtime_identity_validation_tests.rs"]
mod validation_tests;
