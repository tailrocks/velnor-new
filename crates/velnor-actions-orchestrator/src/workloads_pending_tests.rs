//! Unqualified native operations fail before an empty successful workflow exists.

use std::fs;
use velnor_actions_contract::config::{
    AppleAppProfile, NativeDesktopChecks, NativeDesktopProfile, NativeDesktopTarget,
    RustFfiProfile, SwiftTestFramework, WorkloadKind,
};
use velnor_actions_contract::{FileIndex, VelnorConfig, build_index_from_list};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn compiler_role_dispatch_keeps_ordinary_configurations_on_root() -> TestResult {
    use velnor_actions_mise::{PinnedTool, ToolCatalog};
    let root = ToolCatalog::pinned();
    for configuration in ["default", "node_ci", "bun_ci", "ruby_syntax"] {
        let selected = super::catalog_for_configuration(&root, configuration)?;
        assert_eq!(selected.compiler_tool(), PinnedTool::Rust);
        assert_eq!(selected.rustup_toolchain(), root.rustup_toolchain());
    }
    for configuration in ["native_xcode_project_ci", "native_swift_package_ci"] {
        let selected = super::catalog_for_configuration(&root, configuration)?;
        assert_eq!(selected.compiler_tool(), PinnedTool::RustDesktop);
        assert_ne!(selected.rustup_toolchain(), root.rustup_toolchain());
    }
    Ok(())
}

fn orbit_profile() -> NativeDesktopProfile {
    NativeDesktopProfile {
        ffi: RustFfiProfile {
            manifest_path: "components/orbit-bridge/Cargo.toml".to_owned(),
            package: "orbit-bridge".to_owned(),
            profile: "native-release".to_owned(),
            features: vec!["native".to_owned()],
            framework_name: "OrbitCore".to_owned(),
            module_name: "OrbitCoreFFI".to_owned(),
            static_library: "liborbit_bridge.a".to_owned(),
            bindings_path: "generated/orbit-bindings".to_owned(),
            xcframework_path: "build/OrbitCore.xcframework".to_owned(),
        },
        native_root: "clients/orbit".to_owned(),
        target: NativeDesktopTarget::AppleArm64,
        deployment_target: "25.3".to_owned(),
        checks: Some(NativeDesktopChecks {
            source_dirs: vec!["Sources".to_owned()],
            format_config: None,
            lint_config: None,
            swift_test_frameworks: vec![SwiftTestFramework::XCTest],
            cargo_test_packages: vec!["orbit-bridge".to_owned()],
            swift_harness_products: Vec::new(),
        }),
        apple: Some(AppleAppProfile {
            project_spec: "native.yaml".to_owned(),
            project_path: "Orbit.xcodeproj".to_owned(),
            scheme: "OrbitApp".to_owned(),
            app_name: "OrbitApp".to_owned(),
            bundle_identifier: "org.example.orbit".to_owned(),
            bundle_name: "Orbit Desktop".to_owned(),
            app_path: "build/OrbitApp.app".to_owned(),
            derived_data_path: "build/orbit-data".to_owned(),
            archive_name_prefix: "orbit-desktop".to_owned(),
            required_resources: Vec::new(),
            test_target: Some("OrbitVerification".to_owned()),
            ui_test_target: None,
            test_sources: vec!["Verification".to_owned()],
            ui_test_sources: Vec::new(),
            bundle_lsui_element: false,
        }),
    }
}

fn fixture(
    kind: &str,
    paths: &[&str],
) -> Result<(tempfile::TempDir, VelnorConfig, FileIndex), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let native = matches!(kind, "native_xcode_project_ci" | "native_swift_package_ci");
    let base_kind = if native { "docker_build" } else { kind };
    fs::create_dir_all(root.path().join(".velnor"))?;
    fs::write(
        root.path().join(".velnor/config.toml"),
        format!(
            "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[[stacks.workloads]]\nname = \"native\"\nkind = \"{base_kind}\"\n"
        ),
    )?;
    let mut config = crate::config::load_config(root.path())?;
    if native {
        config.stacks.workloads[0].kind = serde_json::from_str(&format!("\"{kind}\""))?;
        config.stacks.workloads[0].native_desktop = Some(orbit_profile());
        config.validate(".velnor/config.toml")?;
    }
    let paths = paths
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    let index = build_index_from_list(root.path(), &paths, &[])?;
    Ok((root, config, index))
}

#[test]
fn complete_desktop_evidence_cannot_emit_empty_green_without_exact_rust_authority() -> TestResult {
    for kind in ["native_xcode_project_ci", "native_swift_package_ci"] {
        let (root, config, index) = fixture(
            kind,
            &[
                "Cargo.toml",
                "Cargo.lock",
                "components/orbit-bridge/Cargo.toml",
                "clients/orbit/Package.swift",
                "clients/orbit/native.yaml",
                "clients/orbit/Sources/Bridge.swift",
                "clients/orbit/Verification/BridgeTests.swift",
            ],
        )?;
        let error =
            super::derive(&config, &index).expect_err("exact desktop Rust authority pending");
        assert!(
            error
                .to_string()
                .contains("desktop_source_bound_primitive_qualification_pending"),
            "{error}"
        );
        assert!(!root.path().join(".github").exists());
    }
    Ok(())
}

#[test]
fn homebrew_cannot_emit_empty_green_without_exact_tool_authority() -> TestResult {
    let (root, config, index) = fixture("homebrew_audit", &["Formula/jackin.rb"])?;
    let error = super::derive(&config, &index).expect_err("Homebrew authority pending");
    assert!(
        error
            .to_string()
            .contains("homebrew_exact_tool_authority_pending"),
        "{error}"
    );
    assert!(!root.path().join(".github").exists());
    Ok(())
}

#[test]
fn desktop_native_swift_coverage_does_not_claim_unrelated_packages() -> TestResult {
    for kind in ["native_xcode_project_ci", "native_swift_package_ci"] {
        let (_, config, covered) = fixture(kind, &["clients/orbit/Package.swift"])?;
        super::evidence::qualify(&config, &covered)?;
        for extra in [
            "Package.swift",
            "other/Package.swift",
            "clients/orbit/other/Package.swift",
            "native/Package.swift",
        ] {
            let (root, config, uncovered) = fixture(kind, &["clients/orbit/Package.swift", extra])?;
            let error = super::evidence::qualify(&config, &uncovered)
                .expect_err("unrelated Swift evidence undeclared");
            assert!(
                error
                    .to_string()
                    .contains(&format!("native_obligation_undeclared:{extra}")),
                "{error}"
            );
            assert!(!root.path().join(".github").exists());
        }
    }
    Ok(())
}

#[test]
fn removed_project_alias_kinds_reject_without_compatibility() {
    for alias in ["jackin_desktop_ci", "jackin_swift_package_ci"] {
        assert!(serde_json::from_str::<WorkloadKind>(&format!("\"{alias}\"")).is_err());
    }
}

#[test]
fn docker_contract_has_no_raw_command_escape() -> TestResult {
    for field in ["command", "args", "run", "script", "shell"] {
        let (root, _, _) = fixture("docker_build", &["Dockerfile"])?;
        fs::write(
            root.path().join(".velnor/config.toml"),
            format!(
                "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[[stacks.workloads]]\nname = \"native\"\nkind = \"docker_build\"\n{field} = \"arbitrary command\"\n"
            ),
        )?;
        let error =
            crate::config::load_config(root.path()).expect_err("raw command field must fail");
        assert!(error.to_string().contains(field), "{error}");
        assert!(!root.path().join(".github").exists());
    }
    Ok(())
}
