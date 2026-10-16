//! Step and command builders for declared Rust test suite jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::{NextestRunIgnored, RustSuiteConfig, Step};

use super::RustSuitesRenderSpec;
use crate::{
    CompileDriver, RenderError, commands,
    matrix::suites::{SUITE_FAIL_FAST_ENV, SUITE_SHARDS_ENV},
    steps,
};

pub(super) fn base_suite_env(rust_version: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), rust_version.to_owned()),
    ])
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
        base_suite_env(&spec.rust_version),
        spec.pull_request_cache_policy,
        None,
    )?;
    Ok(steps.map_or_else(Vec::new, Into::into))
}

pub(super) fn install_suite_tools_step(spec: &RustSuitesRenderSpec) -> Result<Step, RenderError> {
    steps::shell_step(
        "Install suite tools",
        vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "install".to_owned(),
            format!("rust@{}", spec.rust_version),
            format!("cargo-nextest@{}", spec.cargo_nextest_version),
        ],
        base_suite_env(&spec.rust_version),
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
        "mkdir -p \"{archive_dir}\" && mise --no-config --no-env --no-hooks exec rust@{} cargo-nextest@{} -- {driver} nextest archive {}",
        spec.rust_version,
        spec.cargo_nextest_version,
        flags.join(" ")
    );
    let mut env = base_suite_env(&spec.rust_version);
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
        ]
    } else {
        package_and_manifest_flags(suite)?
    };
    if sharded {
        args.push(format!(
            "--partition \"hash:${{{{ matrix.shard }}}}/{}\"",
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
        "mise --no-config --no-env --no-hooks exec rust@{} cargo-nextest@{} -- {driver} nextest run {}",
        spec.rust_version,
        spec.cargo_nextest_version,
        args.join(" ")
    );
    let use_filter_input = standalone_workflow && suite.filter_input.is_some();
    let mut script = build_run_script(&base_cmd, suite.filter.as_deref(), use_filter_input)?;
    if !standalone_workflow && suite.allow_failure {
        script.push_str(" || true");
    }
    let mut env = base_suite_env(&spec.rust_version);
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
    if sharded {
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
    match (has_filterset_input, static_filterset) {
        (false, None) => Ok(base_cmd.to_owned()),
        (false, Some(expr)) => {
            let quoted = commands::quote_run_arg(expr)?;
            Ok(format!("{base_cmd} -E {quoted}"))
        }
        (true, None) => Ok(format!(
            "if [ -n \"${{VELNOR_SUITE_FILTERSET:-}}\" ]; then {base_cmd} -E \"$VELNOR_SUITE_FILTERSET\"; else {base_cmd}; fi"
        )),
        (true, Some(expr)) => {
            let quoted = commands::quote_run_arg(expr)?;
            Ok(format!(
                "if [ -n \"${{VELNOR_SUITE_FILTERSET:-}}\" ]; then {base_cmd} -E \"$VELNOR_SUITE_FILTERSET\"; else {base_cmd} -E {quoted}; fi"
            ))
        }
    }
}
