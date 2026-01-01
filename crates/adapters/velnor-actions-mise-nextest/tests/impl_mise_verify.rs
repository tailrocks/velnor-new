//! `Verify toolchain` step cases (TASK-2.2).
use std::ffi::OsString;
use velnor_actions_contract_release::Finding;
use velnor_actions_mise_catalog::{RouteDriver, ToolCatalog, ToolHomes};
use velnor_actions_mise_core::MiseError;
use velnor_actions_mise_nextest::{TestRunner, VERIFY_TOOLCHAIN_STEP, VerifySpec, VerifyToolchain};

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

fn homes() -> Result<ToolHomes, String> {
    ToolHomes::new("/velnor/rustup", "/velnor/cargo").map_err(|err| err.to_string())
}

fn spec<'a>(
    homes: ToolHomes,
    driver: RouteDriver,
    runner: TestRunner,
    target: &'a str,
    platform: &'a str,
    findings: Vec<Finding>,
) -> VerifySpec<'a> {
    VerifySpec {
        driver,
        runner,
        format: "cargo-1",
        generation: "gen-7",
        target,
        platform,
        homes,
        findings,
    }
}

fn finding() -> Finding {
    Finding {
        code: "missing_recommended_input".to_owned(),
        path: "rust-toolchain.toml".to_owned(),
        observed: None,
        recommended: Some("add an exact channel pin manually".to_owned()),
        action: None,
        reason: "unpinned toolchain input".to_owned(),
    }
}

fn verify(driver: RouteDriver, runner: TestRunner) -> Result<VerifyToolchain, String> {
    let catalog = pinned();
    VerifyToolchain::new(
        &catalog,
        spec(homes()?, driver, runner, "host", "ubuntu-26.04", Vec::new()),
    )
    .map_err(|err| err.to_string())
}

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn verify_step_name_is_contract_fixed() {
    assert_eq!(VERIFY_TOOLCHAIN_STEP, "Verify toolchain");
    assert_eq!(VerifyToolchain::step_name(), VERIFY_TOOLCHAIN_STEP);
}

#[test]
fn verify_selects_cargo_route_with_one_probe() -> Result<(), String> {
    let step = verify(RouteDriver::Cargo, TestRunner::CargoTest)?;
    assert_eq!(step.driver(), RouteDriver::Cargo);
    assert_eq!(step.runner(), TestRunner::CargoTest);
    assert_eq!(step.runner().as_str(), "cargo_test");
    assert_eq!(step.identity_specs(), &["rust@1.98.1".to_owned()]);
    assert_eq!(step.probe_specs(), &["rust@1.98.1".to_owned()]);
    assert!(velnor_actions_contract::is_valid_digest(
        step.cache_format_id()
    ));
    assert_eq!(
        step.probes(&pinned()),
        vec![strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
            "--",
            "cargo",
            "--version",
        ])]
    );
    Ok(())
}

#[test]
fn verify_mbx_route_selects_action_identity_and_rust_probe() -> Result<(), String> {
    let catalog = pinned();
    let step = VerifyToolchain::new(
        &catalog,
        spec(
            homes()?,
            RouteDriver::Mbx,
            TestRunner::CargoTest,
            "x86_64-unknown-linux-gnu",
            "ubuntu-26.04",
            Vec::new(),
        ),
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(
        step.identity_specs(),
        &["rust@1.98.1".to_owned(), "mr-boxington@1.21.1".to_owned()]
    );
    assert_eq!(step.probe_specs(), &["rust@1.98.1".to_owned()]);
    let probes = step.probes(&catalog);
    assert_eq!(probes.len(), 1, "route probe covers cargo_test: {probes:?}");
    assert!(
        !probes[0].iter().any(|arg| arg == "mr-boxington@1.21.1"),
        "Mise must not install or select action-owned MBX: {probes:?}"
    );
    assert!(probes[0].iter().any(|arg| arg == "rust@1.98.1"));
    assert_eq!(probes[0][probes[0].len() - 2], OsString::from("mbx"));
    assert_eq!(probes[0].last(), Some(&OsString::from("--version")));
    assert_eq!(step.target(), "x86_64-unknown-linux-gnu");
    assert_eq!(step.platform(), "ubuntu-26.04");
    Ok(())
}

#[test]
fn verify_nextest_adds_runner_probe_per_driver() -> Result<(), String> {
    let catalog = pinned();
    let cargo = verify(RouteDriver::Cargo, TestRunner::CargoNextest)?;
    assert_eq!(cargo.runner().as_str(), "cargo_nextest");
    assert_eq!(
        cargo.probes(&catalog),
        vec![
            strings(&[
                "mise",
                "--no-config",
                "--no-env",
                "--no-hooks",
                "exec",
                "rust@1.98.1",
                "--",
                "cargo",
                "--version",
            ]),
            strings(&[
                "mise",
                "--no-config",
                "--no-env",
                "--no-hooks",
                "exec",
                "rust@1.98.1",
                "aqua:nextest-rs/nextest/cargo-nextest@0.9.148",
                "--",
                "cargo",
                "nextest",
                "--version",
            ]),
        ]
    );
    let mbx = verify(RouteDriver::Mbx, TestRunner::CargoNextest)?;
    let probes = mbx.probes(&catalog);
    assert_eq!(probes.len(), 2);
    assert_eq!(
        probes[1],
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.148",
            "--",
            "mbx",
            "nextest",
            "--version",
        ])
    );
    assert!(
        !probes[1].iter().any(|arg| arg == "mr-boxington@1.21.1"),
        "Nextest route must use action-owned MBX, not Mise: {probes:?}"
    );
    Ok(())
}

#[test]
fn verify_commands_match_probes_and_env() -> Result<(), String> {
    let catalog = pinned();
    let step = verify(RouteDriver::Cargo, TestRunner::CargoNextest)?;
    let commands = step.commands(&catalog).map_err(|err| err.to_string())?;
    let probes = step.probes(&catalog);
    assert_eq!(commands.len(), probes.len());
    for (command, probe) in commands.iter().zip(probes.iter()) {
        assert_eq!(command.program(), "mise");
        assert_eq!(command.argv(), *probe);
        assert_eq!(command.full_env(), step.env(&catalog));
    }
    let env = step.env(&catalog);
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_RUSTUP_HOME", "/velnor/rustup"),
        ("MISE_CARGO_HOME", "/velnor/cargo"),
        ("RUSTUP_TOOLCHAIN", "1.98.1"),
    ] {
        assert!(
            env.iter()
                .any(|(item_key, item_value)| { item_key == key && item_value == value }),
            "missing {key}={value}: {env:?}"
        );
    }
    assert_eq!(env.len(), 9, "exact step env, no drift: {env:?}");
    Ok(())
}

#[test]
fn verify_unreportable_format_fails_without_guessing() -> Result<(), String> {
    let catalog = pinned();
    for (format, generation) in [("", "gen-7"), ("cargo-1", ""), ("", "")] {
        let mut input = spec(
            homes()?,
            RouteDriver::Cargo,
            TestRunner::CargoTest,
            "host",
            "ubuntu-26.04",
            Vec::new(),
        );
        input.format = format;
        input.generation = generation;
        let err = VerifyToolchain::new(&catalog, input).expect_err("format must fail");
        assert!(
            err.to_string().contains("format_unreportable"),
            "precise miss reason: {err}"
        );
        assert!(matches!(err, MiseError::Contract { .. }));
    }
    Ok(())
}

#[test]
fn verify_rejects_blank_and_hostile_target_or_platform() -> Result<(), String> {
    let catalog = pinned();
    for (target, platform) in [
        ("", "ubuntu-26.04"),
        ("host", ""),
        ("x86_64 unknown", "ubuntu-26.04"),
        ("host", "ubuntu;26.04"),
        ("$(evil)", "ubuntu-26.04"),
    ] {
        let err = VerifyToolchain::new(
            &catalog,
            spec(
                homes()?,
                RouteDriver::Cargo,
                TestRunner::CargoTest,
                target,
                platform,
                Vec::new(),
            ),
        )
        .expect_err("bad target/platform must fail");
        assert!(
            matches!(err, MiseError::InvalidStepInput { .. }),
            "typed rejection: {err}"
        );
    }
    Ok(())
}

#[test]
fn verify_reports_valid_findings_and_rejects_invalid() -> Result<(), String> {
    let catalog = pinned();
    let step = VerifyToolchain::new(
        &catalog,
        spec(
            homes()?,
            RouteDriver::Cargo,
            TestRunner::CargoTest,
            "host",
            "ubuntu-26.04",
            vec![finding()],
        ),
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(step.findings().len(), 1);
    assert_eq!(step.findings()[0].code, "missing_recommended_input");
    assert_eq!(step.findings()[0].path, "rust-toolchain.toml");
    let mut bad = finding();
    bad.code = "Not A Code".to_owned();
    let err = VerifyToolchain::new(
        &catalog,
        spec(
            homes()?,
            RouteDriver::Cargo,
            TestRunner::CargoTest,
            "host",
            "ubuntu-26.04",
            vec![bad],
        ),
    )
    .expect_err("invalid finding must fail");
    assert!(matches!(err, MiseError::Contract { .. }), "{err}");
    Ok(())
}

#[test]
fn verify_finding_without_guidance_is_rejected() -> Result<(), String> {
    let catalog = pinned();
    let mut bad = finding();
    bad.recommended = None;
    let err = VerifyToolchain::new(
        &catalog,
        spec(
            homes()?,
            RouteDriver::Cargo,
            TestRunner::CargoTest,
            "host",
            "ubuntu-26.04",
            vec![bad],
        ),
    )
    .expect_err("guidanceless finding must fail");
    assert!(matches!(err, MiseError::Contract { .. }), "{err}");
    Ok(())
}
