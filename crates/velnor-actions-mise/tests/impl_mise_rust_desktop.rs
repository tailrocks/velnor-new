//! Closed native compiler role and scoped metadata regression cases.

use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::catalog::rust_bootstrap::RustHost;
use velnor_actions_mise::catalog::rust_desktop::DESKTOP_RUST_VERSION;
use velnor_actions_mise::{PinnedTool, RUST_VERSION};

#[test]
fn desktop_scope_preserves_root_pin_and_binds_its_compiler_and_host() -> Result<(), String> {
    let root = ToolCatalog::pinned();
    let desktop = root
        .for_native_kind("native_xcode_project_ci")
        .map_err(|e| e.to_string())?;
    assert_eq!(root.compiler_tool(), PinnedTool::Rust);
    assert_eq!(desktop.compiler_tool(), PinnedTool::RustDesktop);
    assert_eq!(desktop.version(PinnedTool::Rust), RUST_VERSION);
    assert_eq!(desktop.rustup_toolchain(), DESKTOP_RUST_VERSION);
    assert_eq!(desktop.rust_host(), RustHost::MacosArm64);
    assert!(
        desktop
            .tool_spec(desktop.compiler_tool())
            .expect("desktop compiler selector")
            .contains("mr_boxington=true")
    );
    assert!(root.for_native_kind("rust@1.99.0").is_err());
    Ok(())
}

#[test]
fn desktop_owned_components_select_same_compiler_and_mbx_provider() -> Result<(), String> {
    let catalog = ToolCatalog::pinned()
        .for_native_kind("native_swift_package_ci")
        .map_err(|e| e.to_string())?;
    let homes = velnor_actions_mise::ToolHomes::runner_temp();
    let request = velnor_actions_mise::PrepareRustComponents::new(homes.clone());
    let argv = request.argv(&catalog).map_err(|error| error.to_string())?;
    assert!(argv.iter().any(|arg| {
        arg == catalog
            .tool_spec(PinnedTool::RustDesktop)
            .expect("desktop compiler selector")
            .as_str()
    }));
    assert!(argv.iter().any(|arg| {
        arg == catalog
            .tool_spec(PinnedTool::MrBoxington)
            .expect("generic MBX selector")
            .as_str()
    }));
    assert!(argv.iter().any(|arg| arg == "1.99.0-aarch64-apple-darwin"));
    assert!(
        homes
            .env(&catalog)
            .iter()
            .any(|(key, value)| key == "RUSTUP_TOOLCHAIN" && value == "1.99.0")
    );
    Ok(())
}

#[test]
fn metadata_qualification_binds_scoped_compiler_and_keeps_offline_lock() -> Result<(), String> {
    let root = ToolCatalog::pinned();
    let desktop = root
        .for_native_kind("native_xcode_project_ci")
        .map_err(|e| e.to_string())?;
    let request = velnor_actions_mise::MetadataQualification::new("Cargo.toml".into())
        .map_err(|e| e.to_string())?;
    let native = request.argv(&desktop).map_err(|error| error.to_string())?;
    let root_argv = request.argv(&root).map_err(|error| error.to_string())?;
    assert!(native.iter().any(|arg| {
        arg == desktop
            .tool_spec(PinnedTool::RustDesktop)
            .expect("desktop compiler selector")
            .as_str()
    }));
    assert!(native.iter().any(|arg| {
        arg == desktop
            .tool_spec(PinnedTool::MrBoxington)
            .expect("generic MBX selector")
            .as_str()
    }));
    assert!(!native.iter().any(|arg| {
        arg == root
            .tool_spec(PinnedTool::Rust)
            .expect("root compiler selector")
            .as_str()
    }));
    assert!(native.iter().any(|arg| arg == "--locked"));
    assert!(native.iter().any(|arg| arg == "--offline"));
    assert!(root_argv.iter().any(|arg| {
        arg == root
            .tool_spec(PinnedTool::Rust)
            .expect("root compiler selector")
            .as_str()
    }));
    Ok(())
}

#[test]
fn protected_native_source_role_has_no_mbx_dependency() -> Result<(), String> {
    let catalog = ToolCatalog::pinned()
        .for_native_source_kind("native_xcode_project_ci")
        .map_err(|error| error.to_string())?;
    assert_eq!(catalog.compiler_tool(), PinnedTool::RustDesktop);
    assert_eq!(catalog.rustup_toolchain(), "1.99.0");
    assert!(!catalog.rust_uses_mbx());
    assert!(
        !catalog
            .tool_spec(PinnedTool::RustDesktop)
            .expect("desktop compiler selector")
            .contains("mr_boxington")
    );
    assert!(
        catalog
            .tool_spec(PinnedTool::RustDesktop)
            .expect("desktop compiler selector")
            .contains("targets=aarch64-apple-darwin")
    );
    let components = velnor_actions_mise::PrepareRustComponents::new(
        velnor_actions_mise::ToolHomes::runner_temp(),
    );
    assert!(
        !components
            .argv(&catalog)
            .map_err(|error| error.to_string())?
            .iter()
            .any(|arg| {
                arg == catalog
                    .tool_spec(PinnedTool::MrBoxington)
                    .expect("generic MBX selector")
                    .as_str()
            })
    );
    let metadata = velnor_actions_mise::MetadataQualification::new("Cargo.toml".into())
        .map_err(|error| error.to_string())?;
    assert!(
        !metadata
            .argv(&catalog)
            .map_err(|error| error.to_string())?
            .iter()
            .any(|arg| {
                arg == catalog
                    .tool_spec(PinnedTool::MrBoxington)
                    .expect("generic MBX selector")
                    .as_str()
            })
    );
    assert!(
        ToolCatalog::pinned()
            .for_native_source_kind("rust@1.99.0")
            .is_err()
    );
    Ok(())
}

#[test]
fn release_mac_uses_fixed_root_compiler_without_desktop_or_mbx() -> Result<(), String> {
    use velnor_actions_mise::catalog::qualification::DistributionHost;
    let catalog = ToolCatalog::for_release_host(DistributionHost::MacosArm64)
        .map_err(|error| error.to_string())?;
    assert_eq!(catalog.compiler_tool(), PinnedTool::Rust);
    assert_eq!(catalog.rust_host(), RustHost::MacosArm64);
    assert_eq!(catalog.rustup_toolchain(), RUST_VERSION);
    assert_eq!(
        catalog.rust_toolchain_name(),
        format!("{RUST_VERSION}-aarch64-apple-darwin")
    );
    assert!(!catalog.rust_uses_mbx());
    assert!(
        !catalog
            .tool_spec(PinnedTool::Rust)
            .map_err(|error| error.to_string())?
            .contains("mr_boxington")
    );
    let homes = velnor_actions_mise::ToolHomes::runner_temp();
    assert!(
        homes
            .env(&catalog)
            .iter()
            .any(|(key, value)| key == "RUSTUP_TOOLCHAIN" && value == RUST_VERSION)
    );
    for host in [DistributionHost::LinuxAmd64, DistributionHost::LinuxArm64] {
        assert!(ToolCatalog::for_release_host(host).is_err());
    }
    assert_eq!(ToolCatalog::pinned().rust_host(), RustHost::LinuxAmd64);
    Ok(())
}
