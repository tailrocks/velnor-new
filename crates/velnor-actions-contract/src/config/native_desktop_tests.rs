use super::{AppleAppProfile, NativeDesktopProfile, NativeDesktopTarget, RustFfiProfile};
use crate::config::{NativeDesktopChecks, SwiftTestFramework};

fn orbit() -> NativeDesktopProfile {
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
            cargo_test_packages: vec!["orbit-bridge".to_owned()],
            swift_harness_products: vec!["OrbitBehaviorHarness".to_owned()],
            swift_test_frameworks: vec![
                SwiftTestFramework::XCTest,
                SwiftTestFramework::SwiftTesting,
            ],
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
            required_resources: vec!["Contents/Resources/Assets.car".to_owned()],
            test_target: Some("OrbitVerify".to_owned()),
            ui_test_target: Some("OrbitScreens".to_owned()),
            test_sources: vec!["Verification".to_owned()],
            ui_test_sources: vec!["Screens".to_owned()],
            bundle_lsui_element: false,
        }),
    }
}

#[test]
fn generic_profile_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    let profile = orbit();
    profile.validate("config.toml", "workloads.orbit.desktop")?;
    let decoded: NativeDesktopProfile = serde_json::from_str(&serde_json::to_string(&profile)?)?;
    assert_eq!(profile, decoded);
    let mut swift = profile;
    swift.native_root = ".".to_owned();
    swift.apple = None;
    swift.validate("config.toml", "workloads.swift.desktop")?;
    Ok(())
}

#[test]
fn unsafe_paths_features_and_unknown_commands_fail() -> Result<(), Box<dyn std::error::Error>> {
    let mut profile = orbit();
    profile.ffi.manifest_path = "../escape/Cargo.toml".to_owned();
    assert!(profile.validate("config.toml", "desktop").is_err());
    profile = orbit();
    profile.ffi.features = vec!["z".to_owned(), "a".to_owned()];
    assert!(profile.validate("config.toml", "desktop").is_err());
    profile = orbit();
    profile.ffi.features = vec!["a".to_owned(), "a".to_owned()];
    assert!(profile.validate("config.toml", "desktop").is_err());
    let mut json = serde_json::to_value(orbit())?;
    json["command"] = serde_json::Value::String("cargo xtask desktop".to_owned());
    assert!(serde_json::from_value::<NativeDesktopProfile>(json).is_err());
    Ok(())
}

fn jackin() -> NativeDesktopProfile {
    let mut profile = orbit();
    profile.ffi = RustFfiProfile {
        manifest_path: "crates/jackin-usage-ffi/Cargo.toml".to_owned(),
        package: "jackin-usage-ffi".to_owned(),
        profile: "desktop-release".to_owned(),
        features: vec![],
        framework_name: "JackinUsage".to_owned(),
        module_name: "JackinUsageFFI".to_owned(),
        static_library: "libjackin_usage_ffi.a".to_owned(),
        bindings_path: "native/Sources/JackinUsageBindings".to_owned(),
        xcframework_path: "target/xcframework/JackinUsage.xcframework".to_owned(),
    };
    profile.native_root = "native".to_owned();
    profile.deployment_target = "26.0".to_owned();
    profile.checks = Some(NativeDesktopChecks {
        source_dirs: vec![
            "Sources".to_owned(),
            "Tests".to_owned(),
            "UITests".to_owned(),
        ],
        format_config: Some(".swift-format".to_owned()),
        lint_config: Some(".swiftlint.yml".to_owned()),
        cargo_test_packages: vec!["jackin-usage".to_owned(), "jackin-usage-ffi".to_owned()],
        swift_harness_products: vec![
            "DesktopArchitectureLint".to_owned(),
            "DesktopParityMatrixHarness".to_owned(),
            "DesktopSoTParityHarness".to_owned(),
            "ProviderMarksHarness".to_owned(),
            "StatusItemChipHarness".to_owned(),
        ],
        swift_test_frameworks: vec![SwiftTestFramework::XCTest, SwiftTestFramework::SwiftTesting],
    });
    profile.apple = Some(AppleAppProfile {
        project_spec: "project.yml".to_owned(),
        project_path: "JackinDesktop.xcodeproj".to_owned(),
        scheme: "JackinDesktop".to_owned(),
        app_name: "JackinDesktop".to_owned(),
        bundle_identifier: "com.jackin-project.desktop".to_owned(),
        bundle_name: "jackin❯ desktop".to_owned(),
        app_path: "native/dist/JackinDesktop.app".to_owned(),
        derived_data_path: "native/DerivedData".to_owned(),
        archive_name_prefix: "jackin-desktop".to_owned(),
        required_resources: vec!["Contents/Resources/Assets.car".to_owned()],
        test_target: Some("JackinDesktopTests".to_owned()),
        ui_test_target: Some("JackinDesktopUITests".to_owned()),
        test_sources: vec!["Tests/JackinUsageBridgeTests".to_owned()],
        ui_test_sources: vec!["UITests".to_owned()],
        bundle_lsui_element: true,
    });
    profile
}

#[test]
fn distinct_native_consumers_validate() -> Result<(), Box<dyn std::error::Error>> {
    let jackin = jackin();
    let orbit = orbit();
    jackin.validate("config.toml", "jackin")?;
    orbit.validate("config.toml", "orbit")?;
    assert_ne!(jackin.native_root, orbit.native_root);
    assert_ne!(jackin.ffi, orbit.ffi);
    assert_ne!(jackin.apple, orbit.apple);
    assert_ne!(jackin.checks, orbit.checks);
    Ok(())
}

#[test]
fn rejects_unsafe_checks_and_missing_test_sources() {
    let mut profile = orbit();
    profile
        .checks
        .as_mut()
        .expect("fixture checks")
        .format_config = Some("../escape".to_owned());
    assert!(profile.validate("config.toml", "desktop").is_err());
    profile = orbit();
    profile
        .checks
        .as_mut()
        .expect("fixture checks")
        .swift_test_frameworks = vec![SwiftTestFramework::SwiftTesting, SwiftTestFramework::XCTest];
    assert!(profile.validate("config.toml", "desktop").is_err());
    profile = orbit();
    profile
        .apple
        .as_mut()
        .expect("fixture apple")
        .test_sources
        .clear();
    assert!(profile.validate("config.toml", "desktop").is_err());
}

#[test]
fn swift_projection_excludes_rust_execution_scope() -> Result<(), Box<dyn std::error::Error>> {
    use crate::config::SwiftInputs;
    let profile = orbit();
    let inputs = SwiftInputs::from(&profile);
    inputs.validate("config.toml", "swift.consumer")?;
    let mut json = serde_json::to_value(&inputs)?;
    for key in ["manifest_path", "package", "profile", "features"] {
        assert!(json["ffi"].get(key).is_none());
    }
    assert!(json["checks"].get("cargo_test_packages").is_none());
    assert_eq!(serde_json::from_value::<SwiftInputs>(json.clone())?, inputs);
    json["ffi"]["package"] = serde_json::Value::String("foreign-rust-scope".to_owned());
    assert!(serde_json::from_value::<SwiftInputs>(json).is_err());
    let mut unsafe_inputs = inputs;
    unsafe_inputs.ffi.bindings_path = "../foreign".to_owned();
    assert!(
        unsafe_inputs
            .validate("config.toml", "swift.consumer")
            .is_err()
    );
    Ok(())
}

#[test]
fn artifact_basename_identity_is_shared_by_policy_and_consumer() {
    use crate::config::SwiftInputs;
    let mut profile = orbit();
    profile.ffi.xcframework_path = "build/Other.xcframework".to_owned();
    assert!(profile.validate("config.toml", "desktop").is_err());
    assert!(
        SwiftInputs::from(&profile)
            .validate("config.toml", "swift")
            .is_err()
    );
    profile = orbit();
    profile.apple.as_mut().expect("fixture apple").app_path = "build/Other.app".to_owned();
    assert!(profile.validate("config.toml", "desktop").is_err());
    assert!(
        SwiftInputs::from(&profile)
            .validate("config.toml", "swift")
            .is_err()
    );
}
