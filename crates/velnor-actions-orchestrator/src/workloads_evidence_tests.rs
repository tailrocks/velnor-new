//! Independent native profile roots cover exactly one Swift manifest.

use super::*;
use velnor_actions_contract::build_index_from_list;

type TestResult = Result<(), Box<dyn std::error::Error>>;

pub(super) fn profile_workload(
    root: &str,
    native: &str,
) -> Result<WorkloadConfig, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "name": "desktop",
        "kind": "native_swift_package_ci",
        "root": root,
        "native_desktop": {
            "ffi": {
                "manifest_path": "crates/second-ffi/Cargo.toml",
                "package": "second-ffi",
                "profile": "release",
                "framework_name": "SecondCore",
                "module_name": "SecondFFI",
                "static_library": "libsecond_ffi.a",
                "bindings_path": "generated/second",
                "xcframework_path": "build/SecondCore.xcframework"
            },
            "native_root": native,
            "deployment_target": "26.0"
        }
    }))
}

#[test]
fn nested_second_repository_covers_only_declared_native_manifest() -> TestResult {
    let repo = tempfile::TempDir::new()?;
    std::fs::create_dir_all(repo.path().join(".velnor"))?;
    std::fs::write(repo.path().join(".velnor/config.toml"), "schema = 1\n")?;
    let mut config = crate::config::load_config(repo.path())?;
    config
        .stacks
        .workloads
        .push(profile_workload("products/second", "apple/client")?);
    config.validate(".velnor/config.toml")?;
    let exact = "products/second/apple/client/Package.swift".to_owned();
    std::fs::create_dir_all(repo.path().join("products/second/apple/client"))?;
    std::fs::write(
        repo.path().join(&exact),
        "// exact second repository manifest\n",
    )?;
    let index = build_index_from_list(repo.path(), std::slice::from_ref(&exact), &[])?;
    qualify(&config, &index)?;
    for unrelated in [
        "native/Package.swift",
        "products/second/Package.swift",
        "products/second/apple/other/Package.swift",
        "products/other/apple/client/Package.swift",
    ] {
        let path = repo.path().join(unrelated);
        std::fs::create_dir_all(path.parent().ok_or("missing parent")?)?;
        std::fs::write(path, "// unrelated\n")?;
        let index =
            build_index_from_list(repo.path(), &[exact.clone(), unrelated.to_owned()], &[])?;
        let error = qualify(&config, &index).expect_err("unrelated manifest stays undeclared");
        assert!(
            error
                .to_string()
                .contains(&format!("native_obligation_undeclared:{unrelated}"))
        );
    }
    Ok(())
}

#[test]
fn missing_invalid_or_unrelated_profiles_cannot_cover_swift() -> TestResult {
    let workload = profile_workload("products/second", "apple/client")?;
    let exact = "products/second/apple/client/Package.swift";
    assert!(native_profile_covers(&workload, exact));
    let mut missing = workload.clone();
    missing.native_desktop = None;
    assert!(!native_profile_covers(&missing, exact));
    let mut invalid = workload.clone();
    invalid
        .native_desktop
        .as_mut()
        .ok_or("missing profile")?
        .native_root = "../escape".to_owned();
    assert!(!native_profile_covers(
        &invalid,
        "products/second/../escape/Package.swift"
    ));
    let mut unrelated = workload;
    unrelated.kind = WorkloadKind::RubySyntax;
    assert!(!native_profile_covers(&unrelated, exact));
    Ok(())
}
