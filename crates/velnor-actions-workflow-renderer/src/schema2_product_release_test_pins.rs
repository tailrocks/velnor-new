use std::ffi::{OsStr, OsString};

use velnor_actions_contract::ReleaseTarget;
use velnor_actions_mise::catalog::{PinnedTool, ToolCatalog};
use velnor_actions_mise::{MiseInstall, PinnedToolExec, PrepareRustTarget};

use crate::schema2::ProductReleasePins;
use crate::setup::MiseSetup;
use crate::toolchain_env::with_env_unset_argv;

struct BuildArgv {
    product: Vec<String>,
    intel_product: Vec<String>,
    runner: Vec<String>,
    resource_probe: Vec<String>,
}

pub(super) fn test_pins() -> ProductReleasePins {
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
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: install_argv(
            &[
                PinnedTool::Gh,
                PinnedTool::Actionlint,
                PinnedTool::Shellcheck,
                PinnedTool::Zizmor,
            ],
            &catalog,
        ),
        install_build_tools_argv: install_runner_build_tools_argv.clone(),
        install_qualify_tools_argv: install_argv(
            &[
                PinnedTool::Rust,
                PinnedTool::Actionlint,
                PinnedTool::Shellcheck,
                PinnedTool::Zizmor,
            ],
            &catalog,
        ),
        install_runner_build_tools_argv,
        install_gh_argv: install_argv(&[PinnedTool::Gh], &catalog),
        build_argv: build.product,
        intel_build_argv: build.intel_product,
        install_intel_target_argv,
        install_resource_probe_target_argv,
        runner_build_argv: build.runner,
        resource_probe_build_argv: build.resource_probe,
        actionlint_argv: exec_argv(
            &[PinnedTool::Actionlint, PinnedTool::Shellcheck],
            "actionlint",
            &["-color"],
            &catalog,
        ),
        zizmor_argv: with_env_unset_argv(&exec_argv(
            &[PinnedTool::Zizmor],
            "zizmor",
            &[
                "--no-online-audits",
                "--config",
                ".zizmor.yml",
                ".github/workflows",
            ],
            &catalog,
        )),
        gh_argv: exec_argv(&[PinnedTool::Gh], "gh", &[], &catalog),
        rust_version: catalog.version(PinnedTool::Rust).to_owned(),
        mr_boxington_version: catalog.version(PinnedTool::MrBoxington).to_owned(),
    }
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
