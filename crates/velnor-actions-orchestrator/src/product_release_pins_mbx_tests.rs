use super::{install_argv, resource_probe_build_argv};
use crate::mbx_preflight::steps_for_catalog;
use velnor_actions_actionlint::actions::MR_BOXINGTON_ACTION_SHA;
use velnor_actions_contract::StepKind;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

fn strings(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

#[test]
fn product_release_installs_catalog_rust_for_runner_builds() {
    let catalog = ToolCatalog::pinned();
    let argv = install_argv(&[PinnedTool::Rust], &catalog).expect("pinned tool install arguments");
    let actual = strings(&argv);
    let rust = format!("rust@{}", catalog.version(PinnedTool::Rust));

    assert_eq!(
        actual,
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            rust.as_str(),
        ]
    );
}

#[test]
fn linux_resource_probe_build_uses_cargo_and_pinned_rust() {
    let catalog = ToolCatalog::pinned();
    let argv = resource_probe_build_argv(&catalog).expect("pinned resource-probe build arguments");
    let actual = strings(&argv);
    let rust = format!("rust@{}", catalog.version(PinnedTool::Rust));

    assert_eq!(
        actual,
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            rust.as_str(),
            "--",
            "cargo",
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
        ]
    );
}

#[test]
fn release_qualification_uses_catalog_action_and_exact_version_probe() {
    let catalog = ToolCatalog::pinned();
    let steps = steps_for_catalog(
        &catalog,
        velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        None,
    )
    .expect("catalog qualification MBX steps");
    let StepKind::Action { uses, with, .. } = &steps[1].kind else {
        panic!("qualification middle step must install the native MBX action");
    };
    assert_eq!(
        uses,
        &format!("jdx/mr-boxington-action@{MR_BOXINGTON_ACTION_SHA}")
    );
    let mbx_ver = catalog.version(PinnedTool::MrBoxington);
    let rust_ver = catalog.version(PinnedTool::Rust);
    assert_eq!(with.get("version").map(String::as_str), Some(mbx_ver));
    assert_eq!(with.get("toolchain").map(String::as_str), Some(rust_ver));
    let StepKind::Shell { run, .. } = &steps[2].kind else {
        panic!("qualification last step must verify the native MBX version");
    };
    let command = run.join(" ");
    assert!(command.contains("mbx_bin=$(command -v mbx)"));
    assert!(command.contains("case \"$mbx_bin\" in /*)"));
    assert!(command.contains("[ ! -f \"$mbx_bin\" ] || [ ! -x \"$mbx_bin\" ]"));
    assert!(command.contains(&format!(
        "mise --no-config --no-env --no-hooks exec rust@{rust_ver} -- \"$mbx_bin\" --version"
    )));
    assert!(!command.contains("-- mbx --version"));
    assert!(command.contains(&format!("mbx {mbx_ver}")));
}
