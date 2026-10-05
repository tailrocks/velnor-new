//! Report-wrapper recognition for nonexecuting test-binary preparation.
use super::*;

fn task_tool_specs(mbx: bool, nextest: bool) -> Vec<String> {
    let mut tools = vec![format!("rust@{TEST_RUST_TOOLCHAIN}")];
    if mbx {
        tools.push(format!("mr-boxington@{TEST_MBX_VERSION}"));
    }
    if nextest {
        tools.push(nextest_tool_spec());
    }
    tools
}

fn nextest_tool_spec() -> String {
    format!("aqua:nextest-rs/nextest/cargo-nextest@{TEST_NEXTTEST_VERSION}")
}

fn mise_exec_argv(tools: &[String], child: &[&str]) -> Vec<String> {
    let mut argv = ["mise", "--no-config", "--no-env", "--no-hooks", "exec"]
        .map(str::to_owned)
        .to_vec();
    argv.extend(tools.iter().cloned());
    argv.push("--".to_owned());
    argv.extend(child.iter().map(|arg| (*arg).to_owned()));
    argv
}

fn test_build_argv(program: &str, mbx: bool, runner: &str) -> Vec<String> {
    let tools = task_tool_specs(mbx, runner == "nextest");
    let child = if runner == "nextest" {
        vec![
            program,
            "nextest",
            "list",
            "--profile",
            "ci",
            "--list-type",
            "binaries-only",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "demo",
            "--no-default-features",
            "--features",
            "serde",
            "--target",
            "x86_64-unknown-linux-gnu",
        ]
    } else {
        vec![
            program,
            "test",
            "--no-run",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "demo",
        ]
    };
    mise_exec_argv(&tools, &child)
}

fn nextest_run_argv(program: &str, mbx: bool) -> Vec<String> {
    mise_exec_argv(
        &task_tool_specs(mbx, true),
        &[
            program,
            "nextest",
            "run",
            "--profile",
            "ci",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "demo",
        ],
    )
}

fn assert_report_tools(job_id: &str, step: Step, expected: &[String]) {
    let (job_id, mut workflow_job) = job(job_id, "Report wrapper", Vec::new(), vec![step]);
    assert_eq!(infer_job_tools(&workflow_job), expected);
    assert!(
        ensure_setup_p08(
            &job_id,
            &mut workflow_job,
            &mise(),
            false,
            "x86_64-unknown-linux-gnu"
        )
        .is_ok(),
        "exact report wrapper is a valid typed tool source"
    );
}

#[test]
fn typed_test_binary_preparation_uses_exact_nonexecuting_commands() {
    assert_report_tools(
        "report-build-cargo-nextest",
        report_step_for(
            &test_build_argv("cargo", false, "nextest"),
            BUILD_TASK_ID,
            "Build test executables",
        ),
        &[nextest_tool_spec(), format!("rust@{TEST_RUST_TOOLCHAIN}")],
    );
    assert_report_tools(
        "report-build-mbx-nextest",
        report_step_for(
            &test_build_argv("mbx", true, "nextest"),
            BUILD_TASK_ID,
            "Build test executables",
        ),
        &[
            nextest_tool_spec(),
            format!("mr-boxington@{TEST_MBX_VERSION}"),
            format!("rust@{TEST_RUST_TOOLCHAIN}"),
        ],
    );
    assert_report_tools(
        "report-build-cargo-test",
        report_step_for(
            &test_build_argv("cargo", false, "cargo-test"),
            BUILD_TASK_ID,
            "Build test executables",
        ),
        &[format!("rust@{TEST_RUST_TOOLCHAIN}")],
    );
    assert_report_tools(
        "report-build-mbx-test",
        report_step_for(
            &test_build_argv("mbx", true, "cargo-test"),
            BUILD_TASK_ID,
            "Build test executables",
        ),
        &[
            format!("mr-boxington@{TEST_MBX_VERSION}"),
            format!("rust@{TEST_RUST_TOOLCHAIN}"),
        ],
    );
}

#[test]
fn report_wrapper_rejects_development_build_and_wrong_nextest_task() {
    let development_build = mise_exec_argv(
        &task_tool_specs(false, false),
        &[
            "cargo",
            "build",
            "--locked",
            "--offline",
            "--package",
            "demo",
        ],
    );
    assert_setup_rejects(
        report_step_for(&development_build, BUILD_TASK_ID, "Build test executables"),
        "report-build-development-profile",
    );

    let mut test_listing = test_build_argv("cargo", false, "nextest");
    let list_type = test_listing
        .iter_mut()
        .find(|arg| arg.as_str() == "binaries-only")
        .expect("Nextest list type");
    *list_type = "tests".to_owned();
    assert_setup_rejects(
        report_step_for(&test_listing, BUILD_TASK_ID, "Build test executables"),
        "report-build-nextest-test-list",
    );

    let mut wrong_nextest_pin = test_build_argv("cargo", false, "nextest");
    let nextest = wrong_nextest_pin
        .iter_mut()
        .find(|arg| arg.starts_with("aqua:nextest-rs/nextest/cargo-nextest@"))
        .expect("pinned Nextest tool");
    *nextest = "nextest@0.9.146".to_owned();
    assert_setup_rejects(
        report_step_for(&wrong_nextest_pin, BUILD_TASK_ID, "Build test executables"),
        "report-build-nextest-tool-alias",
    );

    assert_report_tools(
        "report-nextest-run",
        report_step_for(
            &nextest_run_argv("mbx", true),
            NEXTEST_TASK_ID,
            "Unit and integration tests",
        ),
        &[
            nextest_tool_spec(),
            format!("mr-boxington@{TEST_MBX_VERSION}"),
            format!("rust@{TEST_RUST_TOOLCHAIN}"),
        ],
    );

    let mut nextest_listing = nextest_run_argv("mbx", true);
    let run = nextest_listing
        .iter_mut()
        .find(|arg| arg.as_str() == "run")
        .expect("Nextest run subcommand");
    *run = "list".to_owned();
    assert_setup_rejects(
        report_step_for(
            &nextest_listing,
            NEXTEST_TASK_ID,
            "Unit and integration tests",
        ),
        "report-nextest-list-is-not-a-test-run",
    );
}
