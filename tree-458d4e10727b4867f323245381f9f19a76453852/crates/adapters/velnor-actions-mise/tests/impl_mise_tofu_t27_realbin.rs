//! T27 real-binary tofu runs (§10 items 2/3/6) via `tofu_exec`.
//!
//! Every run resolves `tofu` through the product
//! `IsolatedCommand::tofu_exec` ctor with the catalog `opentofu@1.13.1`
//! spec — never `mise install`, never ambient `tofu`. The ctor bakes
//! `MISE_AUTO_INSTALL=false` + `MISE_EXEC_AUTO_INSTALL=false`, so a
//! missing tool fails as a preparation error instead of downloading;
//! each run asserts `disables_auto_install()`. Default suite is
//! hermetic: provider-free fixtures, loopback-only failure injection,
//! local signals. Anything needing registry/provider downloads lives
//! behind `#[ignore]` + `VELNOR_LIVE_TOFU=1` (opt-in, documented).
//!
//! Live-spawn cases need a POSIX shell; the whole module is unix-gated.
#![cfg(unix)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use velnor_actions_mise::command::{
    CancelHandle, IsolatedCommand, ProcessOutput, is_cancel_or_timeout,
};
use velnor_actions_mise::{MiseError, OPENTOFU_VERSION, PinnedTool, ToolCatalog};
use velnor_actions_tofu_core::{TofuTaskKind, tofu_payload_argv};

/// Opt-in env var for the network-dependent live test.
const LIVE_ENV: &str = "VELNOR_LIVE_TOFU";
/// Pinned tofu release every real run asserts.
const PINNED_TOFU: &str = "1.13.1";

/// Provider-free fixture: no providers, backends, or modules.
const CLEAN_MAIN_TF: &str =
    "variable \"name\" {\n  type = string\n}\n\noutput \"name\" {\n  value = var.name\n}\n";
/// Malformed sibling for the exit-3 pin.
const BAD_TF: &str = "variable\"name\"{type=string}\n";
/// Provider fixture: real shape, resolved only by the live test.
const PROVIDER_MAIN_TF: &str = "terraform {\n  required_providers {\n    null = {\n      source = \"registry.opentofu.org/hashicorp/null\"\n    }\n  }\n}\n";

/// Staged tofu root plus isolated data/config/cache paths.
struct Stage {
    root: PathBuf,
    data: String,
    config: String,
    cache: String,
}

/// Fresh staging dirs for one test (previous rerun state removed).
fn stage(test: &str) -> Result<Stage, String> {
    let base = std::env::temp_dir().join(format!("velnor-t27-{test}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&base));
    let root = base.join("root");
    let data = base.join("data");
    let cache = base.join("cache");
    for dir in [&root, &data, &cache] {
        std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    }
    let config = base.join("cli.hcl");
    let config_text = format!(
        "plugin_cache_dir = \"{}\"\ndisable_checkpoint = true\n",
        cache.display()
    );
    std::fs::write(&config, config_text).map_err(|err| err.to_string())?;
    Ok(Stage {
        root,
        data: data.display().to_string(),
        config: config.display().to_string(),
        cache: cache.display().to_string(),
    })
}

/// Catalog-derived opentofu spec; asserts the exact pin.
fn opentofu_specs() -> Vec<String> {
    let specs = ToolCatalog::pinned().tool_specs(&[PinnedTool::Opentofu]);
    assert_eq!(specs, ["opentofu@1.13.1".to_owned()], "catalog pin");
    specs
}

/// Product payload argv for one kind at the repo root, driver-prefixed.
///
/// Real runs execute the exact generated payload (`tofu_payload_argv`),
/// never a hand-written approximation, so payload drift fails here.
fn product_argv(kind: TofuTaskKind) -> Result<Vec<OsString>, String> {
    let mut argv = vec![OsString::from("tofu")];
    argv.extend(tofu_payload_argv(kind, "").map_err(|err| err.to_string())?);
    Ok(argv)
}

/// Tofu ctor over one stage from owned argv; asserts install stays disabled.
fn tofu_run_owned(payload: &[OsString], stage: &Stage) -> Result<IsolatedCommand, String> {
    let command = IsolatedCommand::tofu_exec(
        &opentofu_specs(),
        payload,
        &stage.data,
        &stage.config,
        &stage.cache,
    )
    .map_err(|err| err.to_string())?
    .with_cwd(stage.root.clone());
    assert!(
        command.disables_auto_install(),
        "real runs resolve, never install"
    );
    Ok(command)
}

/// Tofu ctor over one stage; asserts install stays disabled.
fn tofu_run(payload: &[&str], stage: &Stage) -> Result<IsolatedCommand, String> {
    let args: Vec<OsString> = payload.iter().map(OsString::from).collect();
    tofu_run_owned(&args, stage)
}

/// Run under explicit bounds; spawn failures surface as strings.
fn run(command: &IsolatedCommand, secs: u64) -> Result<ProcessOutput, String> {
    command
        .run_bounded(1024 * 1024, Duration::from_secs(secs))
        .map_err(|err| err.to_string())
}

/// (6) The catalog pin is exactly `1.13.1` (premise of every run).
#[test]
fn catalog_pin_is_opentofu_1_13_1() {
    assert_eq!(OPENTOFU_VERSION, PINNED_TOFU);
    assert_eq!(opentofu_specs(), ["opentofu@1.13.1".to_owned()]);
}

/// (6) `tofu version` reports the pinned release. Hermetic: local-only.
#[test]
fn real_tofu_version_reports_pinned_release() -> Result<(), String> {
    let stage = stage("version")?;
    let output = run(&tofu_run(&["tofu", "version"], &stage)?, 120)?;
    assert!(output.success);
    assert_eq!(output.code, Some(0));
    let stdout = output.stdout_text("mise").map_err(|err| err.to_string())?;
    assert!(
        stdout.contains("OpenTofu v1.13.1"),
        "pinned release, got {stdout}"
    );
    Ok(())
}

/// (6) Product fmt payload passes a clean provider-free root. Hermetic: files.
#[test]
fn real_tofu_fmt_check_passes_clean_fixture() -> Result<(), String> {
    let stage = stage("fmt-clean")?;
    std::fs::write(stage.root.join("main.tf"), CLEAN_MAIN_TF).map_err(|err| err.to_string())?;
    let output = run(
        &tofu_run_owned(&product_argv(TofuTaskKind::Fmt)?, &stage)?,
        120,
    )?;
    assert!(output.success);
    assert_eq!(output.code, Some(0));
    Ok(())
}

/// (6) Product fmt payload exit 3 stays typed on the real binary. Hermetic.
#[test]
fn real_tofu_fmt_check_exit_3_stays_typed() -> Result<(), String> {
    let stage = stage("fmt-bad")?;
    std::fs::write(stage.root.join("main.tf"), CLEAN_MAIN_TF).map_err(|err| err.to_string())?;
    std::fs::write(stage.root.join("bad.tf"), BAD_TF).map_err(|err| err.to_string())?;
    let output = run(
        &tofu_run_owned(&product_argv(TofuTaskKind::Fmt)?, &stage)?,
        120,
    )?;
    assert!(!output.success);
    assert_eq!(output.code, Some(3));
    assert_eq!(output.signal, None);
    let stdout = output.stdout_text("mise").map_err(|err| err.to_string())?;
    assert!(stdout.contains("bad.tf"), "names the file, got {stdout}");
    assert!(matches!(
        output.require_success("mise"),
        Err(MiseError::NonZeroExit { code: Some(3), .. })
    ));
    Ok(())
}

/// (6) Product init + validate payloads succeed offline on a
/// provider-free root. Hermetic: no providers/backends/modules, backend
/// disabled, checkpoint disabled — nothing to fetch or phone home.
#[test]
fn real_tofu_init_then_validate_offline() -> Result<(), String> {
    let stage = stage("init-validate")?;
    std::fs::write(stage.root.join("main.tf"), CLEAN_MAIN_TF).map_err(|err| err.to_string())?;
    let init = run(
        &tofu_run_owned(&product_argv(TofuTaskKind::InitForValidate)?, &stage)?,
        120,
    )?;
    assert!(
        init.success,
        "init stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert_eq!(init.code, Some(0));
    let validate = run(
        &tofu_run_owned(&product_argv(TofuTaskKind::Validate)?, &stage)?,
        120,
    )?;
    assert!(validate.success);
    assert_eq!(validate.code, Some(0));
    Ok(())
}

/// (2) Provider/network failure is typed on the real binary. Hermetic:
/// the CLI config points at a dead loopback https mirror
/// (`https://127.0.0.1:1/`), so init fails with `connection refused`
/// with or without internet and no download can succeed. NOTE: M4
/// forbids mirrors in product config — this mirror exists only as a
/// negative-test failure injector, never in generated steps. A staged
/// complete lock lets the readonly product init reach the network.
#[test]
fn real_tofu_dead_mirror_init_fails_typed() -> Result<(), String> {
    let stage = stage("dead-mirror")?;
    std::fs::write(stage.root.join("main.tf"), PROVIDER_MAIN_TF).map_err(|err| err.to_string())?;
    std::fs::write(
        stage.root.join(".terraform.lock.hcl"),
        "provider \"registry.opentofu.org/hashicorp/null\" {\n  version = \"3.2.1\"\n  \
         hashes = [\"h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"]\n}\n",
    )
    .map_err(|err| err.to_string())?;
    let mirror = format!(
        "plugin_cache_dir = \"{}\"\ndisable_checkpoint = true\nprovider_installation {{\n  \
         network_mirror {{\n    url = \"https://127.0.0.1:1/\"\n  }}\n}}\n",
        stage.cache
    );
    std::fs::write(&stage.config, mirror).map_err(|err| err.to_string())?;
    let output = run(
        &tofu_run_owned(&product_argv(TofuTaskKind::InitForValidate)?, &stage)?,
        120,
    )?;
    assert!(!output.success);
    assert_eq!(output.code, Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("connection refused"), "got {stderr}");
    let error = output
        .require_success("mise")
        .expect_err("exit 1 must fail");
    assert!(
        matches!(&error, MiseError::NonZeroExit { code: Some(1), stderr: kept, .. }
            if kept == &stderr),
        "code and stderr preserved, got {error}"
    );
    assert!(!is_cancel_or_timeout(&error), "outcome, not abortion");
    Ok(())
}

/// (3) SIGKILL through the tofu ctor preserves the signal. Hermetic:
/// local process only. `mise exec` replaces itself with the payload
/// (probed), so killing `$$` kills the direct child: `code` stays
/// `None` and `signal` reports 9. Extends the generic `repo_task`
/// SIGKILL pin in `impl_mise_isolation` to the tofu ctor.
#[test]
fn real_tofu_payload_sigkill_preserved_through_ctor() -> Result<(), String> {
    let stage = stage("sigkill")?;
    let output = run(&tofu_run(&["/bin/sh", "-c", "kill -9 $$"], &stage)?, 120)?;
    assert!(!output.success);
    assert_eq!(output.code, None);
    assert_eq!(output.signal, Some(9));
    Ok(())
}

/// (3) Runner timeout kills the tofu child and classifies typed.
/// Hermetic: local `sleep` payload, killed well before its deadline.
#[test]
fn real_tofu_timeout_kills_and_classifies() -> Result<(), String> {
    let stage = stage("timeout")?;
    let command = tofu_run(&["/bin/sh", "-c", "sleep 30"], &stage)?;
    let start = std::time::Instant::now();
    let error = command
        .run_cancellable(1024, Duration::from_secs(2), &CancelHandle::new())
        .expect_err("sleep past a 2s deadline must fail");
    assert!(start.elapsed() < Duration::from_secs(30), "child killed");
    assert!(is_cancel_or_timeout(&error), "timeout classifies: {error}");
    assert!(
        error.to_string().contains("timeout_after_secs:2"),
        "got {error}"
    );
    Ok(())
}

/// (6) Fixture shapes pin what may run offline: the default-suite
/// fixtures carry no `required_providers`/`backend`/`module` blocks,
/// while the live fixture does — correctly excluded from default runs.
#[test]
fn offline_fixtures_admit_no_network_shape() {
    for block in ["required_providers", "backend \"", "module \"", "source"] {
        assert!(
            !CLEAN_MAIN_TF.contains(block),
            "clean fixture must not need network: {block}"
        );
    }
    assert!(
        PROVIDER_MAIN_TF.contains("required_providers")
            && PROVIDER_MAIN_TF.contains("registry.opentofu.org/hashicorp/null"),
        "live fixture is provider-shaped, runs only under {LIVE_ENV}=1"
    );
}

/// (6) LIVE: real registry init downloads the null provider through
/// the readonly product payload.
///
/// Ignored by default; runs only with `VELNOR_LIVE_TOFU=1` plus
/// `-- --ignored` (or nextest `--run-ignored`). TOUCHES NETWORK:
/// `registry.opentofu.org` discovery + provider release download into
/// the stage-local cache. Fails loud without the opt-in var so the
/// gate is enforced, never silently skipped. A setup init first writes
/// the lock (lock creation is fixture setup: Velnor never creates
/// locks); the asserted run is the product readonly init over that
/// committed lock, which must succeed without touching it.
#[test]
#[ignore = "needs VELNOR_LIVE_TOFU=1 plus registry network"]
fn live_tofu_registry_init_downloads_provider() -> Result<(), String> {
    if std::env::var(LIVE_ENV).as_deref() != Ok("1") {
        return Err(format!("live test requires {LIVE_ENV}=1"));
    }
    let stage = stage("live")?;
    std::fs::write(stage.root.join("main.tf"), PROVIDER_MAIN_TF).map_err(|err| err.to_string())?;
    let setup = run(
        &tofu_run(
            &[
                "tofu",
                "init",
                "-backend=false",
                "-input=false",
                "-no-color",
            ],
            &stage,
        )?,
        300,
    )?;
    assert!(
        setup.success,
        "setup init stderr: {}",
        String::from_utf8_lossy(&setup.stderr)
    );
    let lock_path = stage.root.join(".terraform.lock.hcl");
    let lock = std::fs::read_to_string(&lock_path).map_err(|err| err.to_string())?;
    assert!(
        lock.contains("registry.opentofu.org/hashicorp/null"),
        "lock pins the downloaded provider: {lock}"
    );
    drop(std::fs::remove_dir_all(&stage.data));
    std::fs::create_dir_all(&stage.data).map_err(|err| err.to_string())?;
    let output = run(
        &tofu_run_owned(&product_argv(TofuTaskKind::InitForValidate)?, &stage)?,
        300,
    )?;
    assert!(
        output.success,
        "live readonly init stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let relocked = std::fs::read_to_string(&lock_path).map_err(|err| err.to_string())?;
    assert_eq!(relocked, lock, "readonly init never rewrites the lock");
    assert!(
        std::path::Path::new(&stage.data)
            .join("providers/registry.opentofu.org/hashicorp/null")
            .is_dir(),
        "provider materialized through the product payload"
    );
    Ok(())
}
