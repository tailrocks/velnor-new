//! Execute the resolver produced by the typed MBX composite emitter.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use velnor_actions_contract::Step;

use crate::verifier_shell::tests::runtime;
use crate::{RenderError, yaml::Yaml};

const LOGICAL: &str = "rust-verifier";
const RUST: &str = "1.99.1";
const ACTION: &str = "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
type TestResult = Result<(), Box<dyn Error>>;

fn context() -> crate::render::RenderContext {
    crate::render::RenderContext {
        tool_pins: Vec::new(),
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        scale_set_image_source_sha: None,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        verified_cache_helpers: BTreeMap::new(),
        require_verified_cache_helper: false,
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: "actions/checkout@0000000000000000000000000000000000000000".to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn typed_mbx_steps() -> Result<Vec<Step>, RenderError> {
    let env = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), RUST.to_owned()),
    ]);
    crate::steps::mbx_steps_for_driver(
        ACTION,
        crate::steps::CompileDriver::Mbx,
        "1.24.1",
        "${VELNOR_TOOL_RUST}",
        env,
        crate::steps::MbxTransport::InstallOnly,
        None,
    )
    .map(|steps| steps.expect("MBX driver creates typed steps"))
}

fn composite_document(
    steps: &[Step],
    ctx: &crate::render::RenderContext,
) -> Result<Yaml, RenderError> {
    velnor_actions_contract::workflow::step_identity::validate_step_identity_scope(
        steps,
        &format!("composite:{LOGICAL}"),
    )
    .map_err(RenderError::Contract)?;
    let empty_job_env = BTreeMap::new();
    let step_context = crate::document_lanes::JobStepContext {
        job_env: &empty_job_env,
        runs_on: None,
        actions_read: false,
    };
    let rendered = steps
        .iter()
        .map(|step| {
            crate::document_steps::step_to_yaml(LOGICAL, step, ctx, &[], true, &step_context)
        })
        .collect::<Result<Vec<_>, _>>()?;
    crate::composite::composite_yaml(LOGICAL, rendered)
}

fn document_steps(document: &Yaml) -> &[Yaml] {
    let runs = map_field(document, "runs");
    let Yaml::Seq(steps) = map_field(runs, "steps") else {
        panic!("composite runs.steps sequence");
    };
    steps
}

fn map_field<'a>(value: &'a Yaml, key: &str) -> &'a Yaml {
    let Yaml::Map(fields) = value else {
        panic!("expected mapping for {key}");
    };
    fields
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn named_step<'a>(steps: &'a [Yaml], name: &str) -> &'a Yaml {
    steps
        .iter()
        .find(|step| matches!(map_field(step, "name"), Yaml::Str(value) if value == name))
        .unwrap_or_else(|| panic!("missing step {name}"))
}

fn write_fake_mise(root: &Path, interpreter: &str) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let data = root.join("mise-data");
    let bin = data.join("bin");
    fs::create_dir_all(&bin)?;
    let executable = bin.join("mise");
    fs::write(
        &executable,
        format!(
            "#!{interpreter}\nset -eu\nprintf '%s|%s|%s\\n' \"$0\" \"$MISE_DATA_DIR\" \"$*\" >> \"$MISE_AUDIT\"\nenv > \"$MISE_ENV_AUDIT\"\ncommand -v env >> \"$MISE_COMMAND_AUDIT\"\ncommand -v sh >> \"$MISE_COMMAND_AUDIT\"\nsh -c 'printf \"nested-shell-ok\\n\"' >> \"$MISE_NESTED_AUDIT\"\ncase \"$*\" in *' config get -f '*) case \"$*\" in *' tools') printf '%s\\n' 'rust = \"1.99.1\"' ;; esac ;; *) exit 93 ;; esac\n"
        ),
    )?;
    let mut permissions = fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions)?;
    Ok((data, executable))
}

fn resolver_step(document: &Yaml) -> &Yaml {
    named_step(
        document_steps(document),
        velnor_actions_contract::config::project_tool_refs::PROJECT_TOOLS_STEP_NAME,
    )
}

fn assert_capsule(
    document: &Yaml,
    file: &crate::tree::RenderedFile,
    ctx: &crate::render::RenderContext,
) -> TestResult {
    let steps = document_steps(document);
    assert_eq!(
        map_field(&steps[0], "name"),
        &Yaml::str(velnor_actions_contract::config::project_tool_refs::PROJECT_TOOLS_STEP_NAME)
    );
    assert_eq!(
        steps.len(),
        4,
        "resolver precedes the complete typed MBX group"
    );
    runtime::assert_emitted_boundary(resolver_step(document));
    assert!(matches!(
        map_field(resolver_step(document), "run"),
        Yaml::Annotated { comment, .. } if comment == "zizmor: ignore[github-env]"
    ));
    let expected =
        crate::marker::with_marker(&ctx.generator_version, &crate::yaml::render_yaml(document))?;
    assert_eq!(
        file.bytes, expected,
        "the executed typed document is the emitted file"
    );
    assert_eq!(file.path, ".github/actions/rust-verifier/action.yml");
    Ok(())
}

fn hostile_environment(
    data: &Path,
    bash_env: &Path,
    marker: &Path,
    root: &Path,
    env_function: &str,
    sh_function: &str,
) -> Vec<(String, String)> {
    let path = |path: &Path| path.to_string_lossy().into_owned();
    vec![
        ("MISE_DATA_DIR".to_owned(), path(data)),
        ("XDG_DATA_HOME".to_owned(), path(&root.join("xdg-decoy"))),
        ("HOME".to_owned(), path(&root.join("home-decoy"))),
        ("BASH_ENV".to_owned(), path(bash_env)),
        ("ENV".to_owned(), path(bash_env)),
        ("BASH_FUNC_env%%".to_owned(), env_function.to_owned()),
        ("BASH_FUNC_sh%%".to_owned(), sh_function.to_owned()),
        ("GITHUB_ENV".to_owned(), path(&root.join("github-env"))),
        ("MISE_AUDIT".to_owned(), path(&root.join("mise-audit"))),
        (
            "MISE_ENV_AUDIT".to_owned(),
            path(&root.join("mise-env-audit")),
        ),
        (
            "MISE_COMMAND_AUDIT".to_owned(),
            path(&root.join("mise-command-audit")),
        ),
        (
            "MISE_NESTED_AUDIT".to_owned(),
            path(&root.join("mise-nested-audit")),
        ),
        ("POISON_LOG".to_owned(), path(marker)),
        ("RUNNER_OS".to_owned(), "Linux".to_owned()),
        ("RUNNER_ARCH".to_owned(), "X64".to_owned()),
        ("GITHUB_TOKEN".to_owned(), "runner-token-canary".to_owned()),
        (
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN".to_owned(),
            "runner-oidc-token-canary".to_owned(),
        ),
        (
            "ACTIONS_ID_TOKEN_REQUEST_URL".to_owned(),
            "runner-oidc-url-canary".to_owned(),
        ),
        (
            "ACTIONS_RUNTIME_TOKEN".to_owned(),
            "runner-runtime-token-canary".to_owned(),
        ),
        (
            "MISE_GITHUB_TOKEN".to_owned(),
            "runner-mise-token-canary".to_owned(),
        ),
        ("GH_TOKEN".to_owned(), "runner-gh-token-canary".to_owned()),
        ("GH_HOST".to_owned(), "runner-gh-host-canary".to_owned()),
        (
            "GH_CONFIG_DIR".to_owned(),
            "runner-gh-config-canary".to_owned(),
        ),
    ]
}

fn assert_runtime(
    root: &Path,
    output: &std::process::Output,
    marker: &Path,
    mise: &Path,
    data: &Path,
) -> TestResult {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let marker_contents = if marker.exists() {
        fs::read_to_string(marker)?
    } else {
        String::new()
    };
    assert!(
        !marker.exists(),
        "hostile startup/function or poisoned PATH ran: {marker_contents}"
    );
    assert_eq!(
        fs::read_to_string(root.join("github-env"))?,
        "VELNOR_TOOL_RUST=1.99.1\n"
    );
    let audit = fs::read_to_string(root.join("mise-audit"))?;
    assert_eq!(
        audit.lines().count(),
        2,
        "only TOML validation and tools lookup run"
    );
    let calls = audit.lines().collect::<Vec<_>>();
    let config_get = format!(
        "{}|{}|--no-config --no-env --no-hooks config get -f ",
        mise.display(),
        data.display()
    );
    assert!(calls[0].starts_with(&config_get));
    assert!(calls[1].starts_with(&config_get) && calls[1].ends_with(" tools"));
    let environment = fs::read_to_string(root.join("mise-env-audit"))?;
    assert!(environment.lines().all(|line| {
        !line.starts_with("BASH_FUNC_")
            && !line.starts_with("BASH_ENV=")
            && !line.starts_with("ENV=")
    }));
    for name in crate::toolchain_env::CREDENTIAL_UNSET_VARS {
        assert!(
            !environment
                .lines()
                .any(|line| line.starts_with(&format!("{name}=")))
        );
    }
    for name in ["CARGO_REGISTRY_TOKEN", "NPM_TOKEN", "NODE_AUTH_TOKEN"] {
        assert!(environment.lines().any(|line| line == format!("{name}=")));
    }
    let commands = fs::read_to_string(root.join("mise-command-audit"))?;
    assert!(
        commands
            .lines()
            .all(|line| line.starts_with("/usr/bin/") || line.starts_with("/bin/"))
    );
    assert_eq!(
        fs::read_to_string(root.join("mise-nested-audit"))?,
        "nested-shell-ok\nnested-shell-ok\n"
    );
    Ok(())
}

fn execute_resolver_case(document: &Yaml, interpreter: &str, hostile: bool) -> TestResult {
    let root = runtime::Scratch::new()?;
    let checkout = root.path().join("checkout");
    fs::create_dir_all(&checkout)?;
    fs::write(checkout.join("mise.toml"), "[tools]\nrust = \"1.99.1\"\n")?;
    fs::write(root.path().join("github-env"), "")?;
    let (data, mise) = write_fake_mise(root.path(), interpreter)?;
    let (marker, bash_env) = runtime::hostile_startup_fixture(root.path())?;
    runtime::poison_path(root.path(), &marker)?;
    let env_function = r#"() { printf '%s\n' function-env >> "$POISON_LOG"; }"#;
    let sh_function = r#"() { printf '%s\n' function-sh >> "$POISON_LOG"; }"#;
    let mut env_values = hostile_environment(
        &data,
        &bash_env,
        &marker,
        root.path(),
        env_function,
        sh_function,
    );
    if !hostile {
        env_values.retain(|(key, _)| {
            !matches!(
                key.as_str(),
                "BASH_ENV" | "ENV" | "BASH_FUNC_env%%" | "BASH_FUNC_sh%%"
            )
        });
    }
    let env = env_values
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    let output =
        runtime::run_emitted_step_in(root.path(), &checkout, resolver_step(document), &env)?;
    assert_runtime(
        root.path(),
        &output,
        &marker,
        &fs::canonicalize(mise)?,
        &fs::canonicalize(data)?,
    )
}

#[test]
fn typed_shared_composite_binds_and_executes_its_hardened_resolver() -> TestResult {
    let steps = typed_mbx_steps()?;
    let ctx = context();
    let document = composite_document(&steps, &ctx)?;
    let mut rebound = document.clone();
    crate::project_tool_resolver::bind_workflow(&mut rebound)?;
    assert_eq!(rebound, document, "composite binding must be idempotent");
    let file = super::composite_file(LOGICAL, &steps, &ctx)?;
    assert_capsule(&document, &file, &ctx)?;
    for (interpreter, hostile) in [("/bin/sh", true), ("/bin/bash", true), ("/bin/bash", false)] {
        execute_resolver_case(&document, interpreter, hostile)?;
    }
    Ok(())
}
