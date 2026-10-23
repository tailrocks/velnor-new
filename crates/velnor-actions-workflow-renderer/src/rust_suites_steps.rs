//! Step and command builders for declared Rust test suite jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::{NextestRunIgnored, RustSuiteConfig, Step};

use super::RustSuitesRenderSpec;
use crate::{
    CompileDriver, RenderError, commands,
    matrix::suites::{SUITE_ALLOW_FAILURE_ENV, SUITE_FAIL_FAST_ENV, SUITE_SHARDS_ENV},
    steps,
};

fn nextest_tool_spec(version: &str) -> String {
    format!("aqua:nextest-rs/nextest/cargo-nextest@{version}")
}

pub(super) fn base_suite_env(spec: &RustSuitesRenderSpec) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
    ]);
    if spec.export_rustup_toolchain {
        env.insert("RUSTUP_TOOLCHAIN".to_owned(), spec.rust_version.clone());
    }
    env
}

pub(super) fn compile_driver_steps(spec: &RustSuitesRenderSpec) -> Result<Vec<Step>, RenderError> {
    let Some((uses, mbx_version)) = &spec.mbx_action else {
        return Ok(Vec::new());
    };
    let steps = steps::mbx_steps_for_driver(
        uses,
        spec.compile_driver,
        mbx_version,
        &spec.rust_version,
        base_suite_env(spec),
        steps::MbxTransport::SuiteBundle,
        None,
    )?;
    Ok(steps.unwrap_or_default())
}

/// Marker after the suite's compile (the whole step when compile and test are one).
pub(super) fn producer_mark_steps(spec: &RustSuitesRenderSpec) -> Result<Vec<Step>, RenderError> {
    if spec.compile_driver == CompileDriver::Mbx {
        Ok(vec![steps::mbx_producer_mark_step()?])
    } else {
        Ok(Vec::new())
    }
}

/// Owned rustup and Cargo homes the toolchain checks read.
fn verify_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
    ])
}

/// Right after the tools cache restore: no restored Rust byte runs unchecked.
pub(super) fn restored_rust_step(spec: &RustSuitesRenderSpec) -> Result<Step, RenderError> {
    let step = crate::verifier_composite::verifier_shell_step(
        velnor_actions_contract::StepRole::VerifyRustManifest,
        "Verify restored Rust toolchain",
        spec.verify_restored_rust_argv.clone(),
        verify_env(),
    )?;
    Ok(step)
}

/// The suite install and, directly after it, the toolchain verification.
pub(super) fn install_suite_tools_steps(
    spec: &RustSuitesRenderSpec,
) -> Result<Vec<Step>, RenderError> {
    let verify = crate::verifier_composite::verifier_shell_step(
        velnor_actions_contract::StepRole::VerifyRustManifest,
        crate::VERIFY_RUST_TOOLCHAIN_STEP,
        spec.verify_rust_argv.clone(),
        verify_env(),
    )?;
    Ok(vec![install_suite_tools_step(spec)?, verify])
}

fn install_suite_tools_step(spec: &RustSuitesRenderSpec) -> Result<Step, RenderError> {
    steps::shell_step(
        "Install suite tools",
        vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "install".to_owned(),
            format!("rust@{}", spec.rust_version),
            nextest_tool_spec(&spec.cargo_nextest_version),
        ],
        base_suite_env(spec),
    )
}

pub(super) fn nextest_archive_step(
    suite: &RustSuiteConfig,
    spec: &RustSuitesRenderSpec,
) -> Result<Step, RenderError> {
    let name = &suite.id;
    let driver = match spec.compile_driver {
        CompileDriver::Cargo => "cargo",
        CompileDriver::Mbx => "mbx",
    };
    let mut flags = package_and_manifest_flags(suite)?;
    let archive_dir = format!("$RUNNER_TEMP/velnor/suite-archive-{name}");
    let archive_file = format!("{archive_dir}/archive.tar.zst");
    flags.push(format!("--archive-file \"{archive_file}\""));
    let script = format!(
        "mkdir -p \"{archive_dir}\" && mise --no-config --no-env --no-hooks exec rust@{} {} -- {driver} nextest archive {}",
        spec.rust_version,
        nextest_tool_spec(&spec.cargo_nextest_version),
        flags.join(" ")
    );
    let mut env = base_suite_env(spec);
    env.extend(suite.env.clone());
    steps::shell_step(
        &format!("Build suite ({name})"),
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )
}

pub(super) fn nextest_run_step(
    suite: &RustSuiteConfig,
    spec: &RustSuitesRenderSpec,
    sharded: bool,
    from_archive: bool,
    standalone_workflow: bool,
) -> Result<Step, RenderError> {
    let name = &suite.id;
    let driver = if from_archive {
        "cargo"
    } else {
        match spec.compile_driver {
            CompileDriver::Cargo => "cargo",
            CompileDriver::Mbx => "mbx",
        }
    };
    let mut args = if from_archive {
        vec![
            format!("--archive-file \"$RUNNER_TEMP/velnor/suite-archive-{name}/archive.tar.zst\""),
            "--workspace-remap .".to_owned(),
            "--extract-to .".to_owned(),
            "--extract-overwrite".to_owned(),
        ]
    } else {
        package_and_manifest_flags(suite)?
    };
    let use_filter_input = standalone_workflow && suite.filter_input.is_some();
    let no_tests = if sharded || suite.filter.is_some() || use_filter_input {
        "warn"
    } else {
        "fail"
    };
    args.push(format!("--no-tests {no_tests}"));
    if sharded {
        args.push(format!(
            "--partition \"hash:$VELNOR_SUITE_SHARD/{}\"",
            suite.partitions
        ));
    }
    args.push(format!(
        "--profile {}",
        commands::quote_run_arg(&suite.profile)?
    ));
    match suite.run_ignored.unwrap_or_default() {
        NextestRunIgnored::Default => {}
        NextestRunIgnored::Only => args.push("--run-ignored only".to_owned()),
        NextestRunIgnored::All => args.push("--run-ignored all".to_owned()),
    }
    let base_cmd = format!(
        "mise --no-config --no-env --no-hooks exec rust@{} {} -- {driver} nextest run {}",
        spec.rust_version,
        nextest_tool_spec(&spec.cargo_nextest_version),
        args.join(" ")
    );
    let script = build_run_script(&base_cmd, suite.filter.as_deref(), use_filter_input)?;
    let mut env = base_suite_env(spec);
    env.extend(suite.env.clone());
    if let Some(threads) = suite.test_threads {
        env.insert("NEXTEST_TEST_THREADS".to_owned(), threads.to_string());
    }
    if standalone_workflow && let Some(input_name) = &suite.filter_input {
        env.insert(
            "VELNOR_SUITE_FILTERSET".to_owned(),
            format!("${{{{ inputs.{input_name} }}}}"),
        );
    }
    if !standalone_workflow && suite.allow_failure {
        env.insert(SUITE_ALLOW_FAILURE_ENV.to_owned(), "true".to_owned());
    }
    if sharded {
        env.insert(
            "VELNOR_SUITE_SHARD".to_owned(),
            "${{ matrix.shard }}".to_owned(),
        );
        env.insert(SUITE_SHARDS_ENV.to_owned(), suite.partitions.to_string());
        env.insert(SUITE_FAIL_FAST_ENV.to_owned(), "false".to_owned());
    }
    steps::shell_step(
        &format!("Run suite ({name})"),
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )
}

fn package_and_manifest_flags(suite: &RustSuiteConfig) -> Result<Vec<String>, RenderError> {
    let mut flags = Vec::new();
    if let Some(manifest) = &suite.manifest_path {
        flags.push(format!(
            "--manifest-path {}",
            commands::quote_run_arg(manifest)?
        ));
    }
    if let Some(pkg) = &suite.package {
        flags.push(format!("--package {}", commands::quote_run_arg(pkg)?));
    } else {
        flags.push("--workspace".to_owned());
    }
    Ok(flags)
}

fn build_run_script(
    base_cmd: &str,
    static_filterset: Option<&str>,
    has_filterset_input: bool,
) -> Result<String, RenderError> {
    let base = match static_filterset {
        Some(expr) => format!("{base_cmd} -E {}", commands::quote_run_arg(expr)?),
        None => base_cmd.to_owned(),
    };
    if has_filterset_input {
        Ok(format!(
            "if [ -n \"${{VELNOR_SUITE_FILTERSET:-}}\" ]; then {base} -E \"$VELNOR_SUITE_FILTERSET\"; else {base}; fi"
        ))
    } else {
        Ok(base)
    }
}

pub(super) fn append_suite_artifact_uploads(
    job_steps: &mut Vec<Step>,
    suite: &RustSuiteConfig,
    sharded: bool,
) -> Result<(), RenderError> {
    let name = &suite.id;
    let id_prefix = format!("{name}-");
    for artifact in &suite.artifacts {
        let resolved_label = artifact
            .name
            .replace("{shard}", if sharded { "${{ matrix.shard }}" } else { "1" })
            .replace(
                "{partition}",
                if sharded { "${{ matrix.shard }}" } else { "1" },
            );
        let has_id_prefix = resolved_label == *name || resolved_label.starts_with(&id_prefix);
        let needs_shard_suffix = sharded && !resolved_label.contains("${{ matrix.shard }}");
        let artifact_name = match (has_id_prefix, needs_shard_suffix) {
            (true, true) => format!(
                "velnor-suite-{}-{resolved_label}-s${{{{ matrix.shard }}}}",
                steps::RUN_KEY_EXPR
            ),
            (true, false) => format!("velnor-suite-{}-{resolved_label}", steps::RUN_KEY_EXPR),
            (false, true) => format!(
                "velnor-suite-{}-{name}-s${{{{ matrix.shard }}}}-{resolved_label}",
                steps::RUN_KEY_EXPR
            ),
            (false, false) => format!(
                "velnor-suite-{}-{name}-{resolved_label}",
                steps::RUN_KEY_EXPR
            ),
        };
        let path = artifact.paths.join("\n");
        job_steps.push(steps::upload_suite_artifact_step(
            &format!("Upload suite artifact ({})", artifact.name),
            &artifact_name,
            &path,
            u32::from(artifact.retention_days),
            artifact.when,
        )?);
    }
    Ok(())
}
