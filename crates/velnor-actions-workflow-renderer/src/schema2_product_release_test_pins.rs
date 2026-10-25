use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use crate::steps::{CompileDriver, mbx_steps_for_driver};
use velnor_actions_contract::{ReleaseTarget, Step};
use velnor_actions_mise::catalog::{PinnedTool, ToolCatalog};
use velnor_actions_mise::{
    ISOLATION_ENV, MISE_CARGO_HOME_ENV, MISE_RUSTUP_HOME_ENV, MiseInstall, NO_AUTO_INSTALL_ENV,
    PinnedToolExec, PrepareRustTarget, RUSTUP_TOOLCHAIN_ENV, ToolHomes,
};

use crate::schema2::ProductReleasePins;
use crate::setup::MiseSetup;

struct BuildArgv {
    product: Vec<String>,
    intel_product: Vec<String>,
    runner: Vec<String>,
    resource_probe: Vec<String>,
}

pub(crate) fn test_pins() -> ProductReleasePins {
    let catalog = ToolCatalog::pinned();
    let setup = MiseSetup {
        uses: format!("jdx/mise-action@{}", "a".repeat(40)),
        version: "2026.10.4".to_owned(),
        sha256: "b".repeat(64),
    };
    let install_runner_build_tools_argv =
        install_argv(&[PinnedTool::Rust, PinnedTool::MrBoxington], &catalog);
    let install_resource_probe_target_argv = resource_probe_target_argv(&catalog);
    let install_intel_target_argv = intel_target_argv(&catalog);
    let build = build_argv(&catalog);
    let lint_argv = lint_argv(&catalog);
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: install_argv(
            &[
                PinnedTool::Gh,
                PinnedTool::Jactionlint,
                PinnedTool::Shellcheck,
            ],
            &catalog,
        ),
        install_build_tools_argv: install_runner_build_tools_argv.clone(),
        install_qualify_tools_argv: install_argv(
            &[
                PinnedTool::Rust,
                PinnedTool::Jactionlint,
                PinnedTool::Shellcheck,
            ],
            &catalog,
        ),
        qualify_mbx_action_steps: qualification_mbx_action_steps(&catalog),
        install_runner_build_tools_argv,
        install_gh_argv: install_argv(&[PinnedTool::Gh], &catalog),
        build_argv: build.product,
        intel_build_argv: build.intel_product,
        install_intel_target_argv,
        install_resource_probe_target_argv,
        runner_build_argv: build.runner,
        resource_probe_build_argv: build.resource_probe,
        install_resource_probe_release_tools_argv: install_argv(
            &[
                PinnedTool::Rust,
                PinnedTool::MrBoxington,
                PinnedTool::Nextest,
            ],
            &catalog,
        ),
        resource_probe_release_build_argv: native_resource_probe_argv(&catalog, false),
        resource_probe_release_test_argv: native_resource_probe_argv(&catalog, true),
        lint_argv,
        verify_lint_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::jactionlint_verifier_script(
                catalog.version(velnor_actions_mise::PinnedTool::Jactionlint),
            ),
        ],
        gh_argv: exec_argv(&[PinnedTool::Gh], "gh", &[], &catalog),
        rust_version: catalog.version(PinnedTool::Rust).to_owned(),
        verify_rust_argv: [
            "sh".to_owned(),
            "-c".to_owned(),
            crate::toolchain_env::with_credential_unset_script("echo verify-rust"),
        ]
        .to_vec(),
        mr_boxington_version: catalog.version(PinnedTool::MrBoxington).to_owned(),
    }
}

fn lint_argv(catalog: &ToolCatalog) -> Vec<String> {
    vec![
        "bash".to_owned(),
        "-c".to_owned(),
        crate::toolchain_env::with_credential_unset_script(
            &velnor_actions_actionlint::workflow_lint_script(
                &exec_argv(
                    &[PinnedTool::Jactionlint, PinnedTool::Shellcheck],
                    "jactionlint",
                    &[],
                    catalog,
                )
                .join(" "),
            ),
        ),
    ]
}

fn qualification_mbx_action_steps(catalog: &ToolCatalog) -> [Step; 3] {
    let homes = ToolHomes::runner_temp();
    let mut env = ISOLATION_ENV
        .iter()
        .chain(NO_AUTO_INSTALL_ENV.iter())
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect::<BTreeMap<_, _>>();
    env.insert(
        MISE_RUSTUP_HOME_ENV.to_owned(),
        homes.rustup_home().to_owned(),
    );
    env.insert(
        MISE_CARGO_HOME_ENV.to_owned(),
        homes.cargo_home().to_owned(),
    );
    env.insert("RUSTUP_HOME".to_owned(), homes.rustup_home().to_owned());
    env.insert("CARGO_HOME".to_owned(), homes.cargo_home().to_owned());
    env.insert(RUSTUP_TOOLCHAIN_ENV.to_owned(), catalog.rustup_toolchain());
    mbx_steps_for_driver(
        &format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
        CompileDriver::Mbx,
        catalog.version(PinnedTool::MrBoxington),
        catalog.version(PinnedTool::Rust),
        env,
        crate::steps::MbxTransport::InstallOnly,
        None,
    )
    .expect("test MBX qualification steps are valid")
    .and_then(|steps| <[Step; 3]>::try_from(steps).ok())
    .expect("MBX qualification emits the native action steps")
}

fn resource_probe_target_argv(catalog: &ToolCatalog) -> Vec<String> {
    target_argv(
        ReleaseTarget::LinuxX86_64,
        "x86_64-unknown-linux-musl",
        catalog,
    )
}

fn intel_target_argv(catalog: &ToolCatalog) -> Vec<String> {
    target_argv(
        ReleaseTarget::MacosArm64,
        ReleaseTarget::MacosX86_64.triple(),
        catalog,
    )
}

fn build_argv(catalog: &ToolCatalog) -> BuildArgv {
    let mut product_args = vec![
        "build",
        "--release",
        "--locked",
        "--package",
        "velnor-actions-cli",
        "--bin",
        "velnor-actions",
    ];
    let product = mbx_build_argv(&product_args, catalog);
    product_args.extend(["--target", ReleaseTarget::MacosX86_64.triple()]);
    let intel_product = mbx_build_argv(&product_args, catalog);
    let resource_probe = mbx_build_argv(
        &[
            "build",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--package",
            "velnor-resource-probe",
            "--bin",
            "velnor-resource-probe",
            "--release",
            "--target",
            "x86_64-unknown-linux-musl",
        ],
        catalog,
    );
    let runner = mbx_build_argv(
        &[
            "build",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--release",
            "--package",
            "velnor-runner-cli",
            "--bin",
            "velnor-host",
        ],
        catalog,
    );
    BuildArgv {
        product,
        intel_product,
        runner,
        resource_probe,
    }
}

fn install_argv(tools: &[PinnedTool], catalog: &ToolCatalog) -> Vec<String> {
    MiseInstall::new(tools.to_vec())
        .expect("test tool list is non-empty")
        .argv(catalog)
        .into_iter()
        .map(|arg| arg.into_string().expect("Mise argv is UTF-8"))
        .collect()
}

fn mbx_build_argv(args: &[&str], catalog: &ToolCatalog) -> Vec<String> {
    exec_argv(
        &[PinnedTool::Rust, PinnedTool::MrBoxington],
        "mbx",
        args,
        catalog,
    )
}

fn exec_argv(
    tools: &[PinnedTool],
    program: &str,
    args: &[&str],
    catalog: &ToolCatalog,
) -> Vec<String> {
    PinnedToolExec::new(
        tools.to_vec(),
        OsStr::new(program),
        args.iter().map(OsString::from).collect(),
    )
    .expect("test MBX argv is valid")
    .argv(catalog)
    .into_iter()
    .map(|arg| arg.into_string().expect("Mise argv is UTF-8"))
    .collect()
}

fn target_argv(host: ReleaseTarget, target: &str, catalog: &ToolCatalog) -> Vec<String> {
    PrepareRustTarget::new(host.triple(), target)
        .expect("test Rust target pair is valid")
        .argv(catalog)
        .into_iter()
        .map(|arg| arg.into_string().expect("Mise argv is UTF-8"))
        .collect()
}

fn native_resource_probe_argv(catalog: &ToolCatalog, tests: bool) -> Vec<String> {
    let args = if tests {
        vec![
            "nextest",
            "run",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--package",
            "velnor-runner-host",
            "--lib",
            "--target",
            "x86_64-unknown-linux-gnu",
            "-E",
            "test(artifact_admission::archive) | test(resource_probe_release)",
        ]
    } else {
        vec![
            "build",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--package",
            "velnor-runner-host",
            "--bin",
            "velnor-resource-probe-release",
            "--release",
            "--target",
            "x86_64-unknown-linux-gnu",
        ]
    };
    exec_argv(
        &[
            PinnedTool::Rust,
            PinnedTool::MrBoxington,
            PinnedTool::Nextest,
        ],
        "mbx",
        &args,
        catalog,
    )
}
