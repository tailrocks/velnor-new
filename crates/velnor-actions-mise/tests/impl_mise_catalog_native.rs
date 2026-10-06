//! Explicit-host selector and native workload cases.
use velnor_actions_mise::catalog::{qualification::DistributionHost, workload::WorkloadExec};
use velnor_actions_mise::{MiseError, PinnedTool, ToolCatalog};

#[test]
fn native_tools_use_exact_backend_selectors() {
    let catalog = ToolCatalog::pinned();
    for (tool, selector) in [
        (PinnedTool::Swift, "swift@6.4.0"),
        (PinnedTool::Ruby, "ruby@4.0.7"),
        (
            PinnedTool::Reuse,
            "pipx:reuse[extras=charset-normalizer,uvx_args=\"--python 3.14.8 --no-python-downloads\"]@6.2.0",
        ),
        (PinnedTool::CargoAudit, "cargo:cargo-audit@0.22.2"),
        (PinnedTool::CargoDeny, "cargo-deny@0.20.2"),
        (PinnedTool::Alint, "github:asamarts/alint@0.17.0"),
        (PinnedTool::Boltffi, "github:boltffi/boltffi@0.30.1"),
        (PinnedTool::Xcodegen, "xcodegen@2.46.0"),
        (PinnedTool::Jq, "jq@1.8.2"),
        (PinnedTool::SwiftLint, "aqua:realm/SwiftLint@0.65.1"),
        (PinnedTool::Periphery, "aqua:peripheryapp/periphery@3.8.0"),
    ] {
        assert_eq!(catalog.tool_spec(tool).expect("generic selector"), selector);
    }
    let host = DistributionHost::MacosArm64;
    for tool in [
        PinnedTool::Bun,
        PinnedTool::Node,
        PinnedTool::Opentofu,
        PinnedTool::Python,
        PinnedTool::Uv,
        PinnedTool::Java,
        PinnedTool::Gradle,
    ] {
        let record = catalog
            .native_distribution(host, tool)
            .expect("qualified native fixture");
        assert_eq!(
            catalog
                .native_tool_spec(host, tool)
                .expect("qualified native selector"),
            record.selector(),
            "native selector must come from the qualified host record: {tool:?}"
        );
    }
}

#[test]
fn native_workload_directory_and_no_auto_install_survive_rendering() -> Result<(), MiseError> {
    let request = WorkloadExec::new(
        "docs",
        vec![PinnedTool::Bun],
        ["bun", "run", "build"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect(),
    )?;
    let catalog = ToolCatalog::pinned();
    let host = DistributionHost::MacosArm64;
    let selector = catalog.native_tool_spec(host, PinnedTool::Bun)?;
    let command = request.command_for_host(&catalog, host)?;
    assert!(command.disables_auto_install());
    assert_eq!(
        request.argv_for_host(&catalog, host)?,
        [
            "mise",
            "--cd",
            "docs",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            selector.as_str(),
            "--",
            "bun",
            "run",
            "build"
        ]
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>()
    );
    for root in [
        "",
        "../docs",
        "/docs",
        "docs/../native",
        "-C",
        "docs//native",
    ] {
        assert!(WorkloadExec::new(root, vec![], vec![std::ffi::OsString::from("docker")]).is_err());
    }
    assert!(WorkloadExec::new(".", vec![], vec![std::ffi::OsString::from("rustup")]).is_err());
    Ok(())
}
