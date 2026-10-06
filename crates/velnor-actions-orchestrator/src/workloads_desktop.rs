//! Closed native FFI, Swift package and Xcode project validation primitives.

use velnor_actions_contract::FileIndex;
use velnor_actions_contract::config::{NativeDesktopProfile, WorkloadConfig, WorkloadKind};
use velnor_actions_mise::PinnedTool;

use crate::OrchestratorError;

pub(crate) fn validate_evidence(
    workload: &WorkloadConfig,
    index: &FileIndex,
) -> Result<(), OrchestratorError> {
    if kind_id(workload.kind).is_empty() {
        return Ok(());
    }
    let profile = profile(workload)?;
    let mut required = vec!["Cargo.lock".to_owned(), profile.ffi.manifest_path.clone()];
    match workload.kind {
        WorkloadKind::NativeSwiftPackageCi => required.push(native_path(profile, "Package.swift")),
        WorkloadKind::NativeXcodeProjectCi => {
            let apple = profile.apple.as_ref().ok_or_else(missing_apple)?;
            required.push(native_path(profile, &apple.project_spec));
        }
        _ => return Ok(()),
    }
    for path in required {
        let relative = if workload.root.as_str() == "." {
            path
        } else {
            format!("{}/{path}", workload.root.as_str())
        };
        if !index.contains(&relative) {
            return Err(crate::internal::internal(&format!(
                "native_desktop_evidence_missing:{relative}"
            )));
        }
    }
    Ok(())
}

pub(crate) const fn kind_id(kind: WorkloadKind) -> &'static str {
    match kind {
        WorkloadKind::NativeXcodeProjectCi => "native_xcode_project_ci",
        WorkloadKind::NativeSwiftPackageCi => "native_swift_package_ci",
        _ => "",
    }
}

/// Native Swift belongs to the selected Xcode installation.
pub(crate) fn tools(kind: WorkloadKind) -> Vec<PinnedTool> {
    let mut tools = match kind {
        WorkloadKind::NativeXcodeProjectCi | WorkloadKind::NativeSwiftPackageCi => vec![
            PinnedTool::RustDesktop,
            PinnedTool::MrBoxington,
            PinnedTool::Boltffi,
        ],
        _ => return Vec::new(),
    };
    if kind == WorkloadKind::NativeXcodeProjectCi {
        tools.push(PinnedTool::Xcodegen);
    }
    tools
}

pub(crate) fn phases(
    workload: &WorkloadConfig,
    source_sha: &str,
) -> Result<Vec<(&'static str, Vec<String>)>, OrchestratorError> {
    if kind_id(workload.kind).is_empty() {
        return Ok(Vec::new());
    }
    let profile = profile(workload)?;
    let stages: &[(&'static str, &'static str)] = match workload.kind {
        WorkloadKind::NativeXcodeProjectCi => {
            profile.apple.as_ref().ok_or_else(missing_apple)?;
            &[
                ("native-ffi", "ffi"),
                ("native-generate", "generate-project"),
                ("native-xcode-build", "xcode-build"),
                ("native-xcode-test", "xcode-test"),
            ]
        }
        WorkloadKind::NativeSwiftPackageCi => &[
            ("native-ffi", "ffi"),
            ("native-swift-build", "swift-build"),
            ("native-swift-test", "swift-test"),
        ],
        _ => return Ok(Vec::new()),
    };
    let profile_file = format!(".github/velnor/desktop/workloads/{}.json", workload.name);
    Ok(stages
        .iter()
        .map(|(phase, stage)| {
            command(
                phase,
                &[
                    "python3",
                    ".github/velnor/desktop/desktop_native.py",
                    stage,
                    "--profile",
                    &profile_file,
                    "--source-root",
                    workload.root.as_str(),
                    "--source-sha",
                    source_sha,
                ],
            )
        })
        .collect())
}

fn profile(workload: &WorkloadConfig) -> Result<&NativeDesktopProfile, OrchestratorError> {
    workload
        .native_desktop
        .as_ref()
        .ok_or_else(|| crate::internal::internal("native_desktop_profile_missing"))
}

fn missing_apple() -> OrchestratorError {
    crate::internal::internal("native_desktop_apple_profile_missing")
}

fn native_path(profile: &NativeDesktopProfile, path: &str) -> String {
    if profile.native_root == "." {
        path.to_owned()
    } else {
        format!("{}/{path}", profile.native_root)
    }
}

fn command(phase: &'static str, arguments: &[&str]) -> (&'static str, Vec<String>) {
    (
        phase,
        arguments.iter().map(|value| (*value).to_owned()).collect(),
    )
}

pub(crate) fn rank(phase: &str) -> Option<u32> {
    match phase {
        "native-ffi" => Some(0),
        "native-generate" => Some(1),
        "native-xcode-build" | "native-swift-build" => Some(2),
        "native-xcode-test" | "native-swift-test" => Some(3),
        _ => None,
    }
}

pub(crate) fn step_name(phase: &str) -> Option<&'static str> {
    match phase {
        "native-ffi" => Some("Build native FFI XCFramework"),
        "native-generate" => Some("Generate native Xcode project"),
        "native-xcode-build" => Some("Build native Apple app"),
        "native-xcode-test" => Some("Test native Apple app"),
        "native-swift-build" => Some("Build native Swift package"),
        "native-swift-test" => Some("Test native Swift package"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::config::{
        AppleAppProfile, NativeDesktopChecks, NativeDesktopTarget, RustFfiProfile,
        SwiftTestFramework,
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

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
            checks: Some(NativeDesktopChecks {
                source_dirs: vec!["AppSources".to_owned(), "Verification".to_owned()],
                format_config: Some("config/formatter.json".to_owned()),
                lint_config: Some("config/lint.yaml".to_owned()),
                swift_test_frameworks: vec![
                    SwiftTestFramework::XCTest,
                    SwiftTestFramework::SwiftTesting,
                ],
                cargo_test_packages: vec!["orbit-bridge".to_owned()],
                swift_harness_products: vec!["OrbitBehaviorHarness".to_owned()],
            }),
            target: NativeDesktopTarget::AppleArm64,
            deployment_target: "25.3".to_owned(),
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
                test_target: Some("OrbitVerify".to_owned()),
                ui_test_target: Some("OrbitScreens".to_owned()),
                test_sources: vec!["Verification".to_owned()],
                ui_test_sources: vec!["Screens".to_owned()],
                bundle_lsui_element: false,
            }),
        }
    }

    fn workload(kind: WorkloadKind) -> WorkloadConfig {
        WorkloadConfig {
            name: "orbit".to_owned(),
            kind,
            root: velnor_actions_contract::config::Utf8RepoRelDir::from_raw(
                "apps/orbit".to_owned(),
            ),
            inputs: Vec::new(),
            paths: Vec::new(),
            scripts: None,
            gradle: None,
            package_update: None,
            native_desktop: Some(orbit_profile()),
        }
    }

    #[test]
    fn nested_workload_requires_relative_lock_ffi_manifest_and_native_evidence() -> TestResult {
        let root = tempfile::TempDir::new()?;
        for (kind, native) in [
            (
                WorkloadKind::NativeXcodeProjectCi,
                "clients/orbit/native.yaml",
            ),
            (
                WorkloadKind::NativeSwiftPackageCi,
                "clients/orbit/Package.swift",
            ),
        ] {
            let paths: Vec<_> = ["Cargo.lock", "components/orbit-bridge/Cargo.toml", native]
                .map(|path| format!("apps/orbit/{path}"))
                .to_vec();
            let complete =
                velnor_actions_contract::build_index_from_list(root.path(), &paths, &[])?;
            validate_evidence(&workload(kind), &complete)?;
            for missing in &paths {
                let subset: Vec<_> = paths
                    .iter()
                    .filter(|path| *path != missing)
                    .cloned()
                    .collect();
                let index =
                    velnor_actions_contract::build_index_from_list(root.path(), &subset, &[])?;
                let error =
                    validate_evidence(&workload(kind), &index).expect_err("evidence missing");
                assert!(error.to_string().contains(missing), "{error}");
            }
        }
        Ok(())
    }

    #[test]
    fn generated_ffi_primitive_precedes_native_validation_without_repository_dispatch() -> TestResult
    {
        for kind in [
            WorkloadKind::NativeXcodeProjectCi,
            WorkloadKind::NativeSwiftPackageCi,
        ] {
            let phases = phases(&workload(kind), SOURCE_SHA)?;
            assert_eq!(
                phases[0].1,
                [
                    "python3",
                    ".github/velnor/desktop/desktop_native.py",
                    "ffi",
                    "--profile",
                    ".github/velnor/desktop/workloads/orbit.json",
                    "--source-root",
                    "apps/orbit",
                    "--source-sha",
                    SOURCE_SHA
                ]
            );
            for pair in phases.windows(2) {
                assert!(rank(pair[0].0) < rank(pair[1].0));
            }
            for (phase, vector) in phases {
                assert!(step_name(phase).is_some());
                assert!(velnor_actions_workflow_renderer::validate_command_argv(&vector).is_ok());
                assert!(
                    !vector.iter().any(|arg| matches!(
                        arg.as_str(),
                        "sh" | "bash" | "mise" | "cargo" | "xtask"
                    ))
                );
            }
            assert!(tools(kind).contains(&PinnedTool::RustDesktop));
            assert!(!tools(kind).contains(&PinnedTool::Swift));
        }
        Ok(())
    }

    #[test]
    fn swift_uses_profile_package_without_xcode_project() -> TestResult {
        let mut workload = workload(WorkloadKind::NativeSwiftPackageCi);
        workload
            .native_desktop
            .as_mut()
            .ok_or("profile absent")?
            .apple = None;
        let phases = phases(&workload, SOURCE_SHA)?;
        assert_eq!(phases[1].1[2], "swift-build");
        assert_eq!(phases[2].1[2], "swift-test");
        Ok(())
    }

    #[test]
    fn apple_fixed_stages_preserve_generation_build_and_test_order() -> TestResult {
        let phases = phases(&workload(WorkloadKind::NativeXcodeProjectCi), SOURCE_SHA)?;
        for (index, action) in [
            (1, "generate-project"),
            (2, "xcode-build"),
            (3, "xcode-test"),
        ] {
            assert_eq!(phases[index].1[2], action);
            assert_eq!(
                phases[index].1[4],
                ".github/velnor/desktop/workloads/orbit.json"
            );
            assert_eq!(phases[index].1[6], "apps/orbit");
            assert_eq!(phases[index].1[8], SOURCE_SHA);
        }
        Ok(())
    }

    #[test]
    fn missing_native_or_apple_profile_fails_before_materializing_commands() -> TestResult {
        let mut workload = workload(WorkloadKind::NativeXcodeProjectCi);
        workload
            .native_desktop
            .as_mut()
            .ok_or("profile absent")?
            .apple = None;
        assert!(
            phases(&workload, SOURCE_SHA)
                .expect_err("apple profile required")
                .to_string()
                .contains("apple_profile_missing")
        );
        workload.native_desktop = None;
        assert!(
            phases(&workload, SOURCE_SHA)
                .expect_err("native profile required")
                .to_string()
                .contains("native_desktop_profile_missing")
        );
        Ok(())
    }
}
