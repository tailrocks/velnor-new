//! Hostless native requests never bypass qualified installation records.

use std::ffi::{OsStr, OsString};
use velnor_actions_mise::catalog::{
    CARGO_SEMVER_CHECKS_VERSION,
    qualification::{CARGO_SEMVER_CHECKS_SELECTION_VERSION, DistributionHost, DistributionTool},
    workload::WorkloadExec,
};
use velnor_actions_mise::{MiseInstall, PinnedTool, PinnedToolExec, ToolCatalog};

const NATIVE_TOOLS: [PinnedTool; 10] = [
    PinnedTool::Gh,
    PinnedTool::Bun,
    PinnedTool::Node,
    PinnedTool::Opentofu,
    PinnedTool::Python,
    PinnedTool::Uv,
    PinnedTool::Java,
    PinnedTool::Gradle,
    PinnedTool::CargoSemverChecks,
    PinnedTool::ReleasePlz,
];

#[test]
fn native_catalog_slots_require_explicit_host() {
    let catalog = ToolCatalog::pinned();
    for tool in NATIVE_TOOLS {
        assert!(catalog.tool_spec(tool).is_err(), "{tool:?}");
        assert!(catalog.tool_specs(&[PinnedTool::Actionlint, tool]).is_err());
    }
    assert_eq!(
        catalog.tool_spec(PinnedTool::Actionlint),
        Ok(format!(
            "actionlint@{}",
            catalog.version(PinnedTool::Actionlint)
        ))
    );
}

#[test]
fn hostless_install_exec_and_workload_reject_native_selection() {
    let catalog = ToolCatalog::pinned();
    let install = MiseInstall::new(vec![PinnedTool::Java]).expect("valid tool set");
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Java],
        OsStr::new("java"),
        vec![OsString::from("--version")],
    )
    .expect("valid payload");
    let workload = WorkloadExec::new(
        "project",
        vec![PinnedTool::Java],
        vec![OsString::from("java"), OsString::from("--version")],
    )
    .expect("valid workload");
    assert!(install.argv(&catalog).is_err());
    assert!(install.command(&catalog).is_err());
    assert!(exec.argv(&catalog).is_err());
    assert!(exec.command(&catalog).is_err());
    assert!(workload.command(&catalog).is_err());
}

#[test]
fn native_requests_use_exact_installation_selector_and_reject_unmeasured_host() {
    let catalog = ToolCatalog::pinned();
    for tool in [
        PinnedTool::Java,
        PinnedTool::Gradle,
        PinnedTool::CargoSemverChecks,
    ] {
        let record = catalog
            .native_distribution(DistributionHost::MacosArm64, tool)
            .expect("qualified native installation");
        assert_eq!(
            catalog.native_tool_spec(DistributionHost::MacosArm64, tool),
            Ok(record.selector().to_owned())
        );
        let install = MiseInstall::new(vec![tool]).expect("valid tool set");
        let argv = install
            .argv_for_host(&catalog, DistributionHost::MacosArm64)
            .expect("qualified install");
        assert!(argv.iter().any(|arg| arg == record.selector()));
        for host in [DistributionHost::LinuxAmd64, DistributionHost::LinuxArm64] {
            assert!(catalog.native_tool_spec(host, tool).is_err());
            assert!(install.command_for_host(&catalog, host).is_err());
        }
    }
}

#[test]
fn explicit_host_cannot_reinterpret_the_compiler_role() {
    let root = ToolCatalog::pinned();
    assert!(
        root.native_tool_spec(DistributionHost::MacosArm64, PinnedTool::Rust)
            .is_err()
    );
    assert!(
        root.native_tool_spec(DistributionHost::MacosArm64, PinnedTool::RustDesktop)
            .is_err()
    );
    let desktop = root
        .for_native_kind("native_xcode_project_ci")
        .expect("closed desktop role");
    assert!(
        desktop
            .native_tool_spec(DistributionHost::MacosArm64, PinnedTool::RustDesktop)
            .is_ok()
    );
    assert!(
        desktop
            .native_tool_spec(DistributionHost::LinuxAmd64, PinnedTool::RustDesktop)
            .is_err()
    );
}

#[test]
fn semver_slot_uses_only_the_closed_distribution_authority() {
    let catalog = ToolCatalog::pinned();
    let tool = PinnedTool::CargoSemverChecks;
    assert_eq!(PinnedTool::from_tool_name("cargo-semver-checks"), Ok(tool));
    assert!(PinnedTool::from_tool_name("semver-checks").is_err());
    assert_eq!(
        CARGO_SEMVER_CHECKS_VERSION,
        CARGO_SEMVER_CHECKS_SELECTION_VERSION
    );
    assert_eq!(catalog.version(tool), CARGO_SEMVER_CHECKS_SELECTION_VERSION);
    let record = catalog
        .native_distribution(DistributionHost::MacosArm64, tool)
        .expect("canonical actual-host HTTP qualification");
    assert_eq!(record.tool(), DistributionTool::CargoSemverChecks);
    assert!(record.selector().starts_with("http:cargo-semver-checks["));
    assert!(record.required_install_plan().is_ok());
    let identity = catalog.tool_identity(tool);
    assert_eq!(identity.platforms, ["macos-26"]);
    assert_eq!(
        identity.source,
        format!(
            "{}/tree/{}",
            record.source_repository(),
            record.source_commit()
        )
    );
    let install = MiseInstall::new(vec![tool]).expect("closed tool set");
    let exec = PinnedToolExec::new(vec![tool], OsStr::new("cargo-semver-checks"), vec![])
        .expect("closed executable request");
    assert!(install.argv(&catalog).is_err());
    assert!(exec.argv(&catalog).is_err());
}
