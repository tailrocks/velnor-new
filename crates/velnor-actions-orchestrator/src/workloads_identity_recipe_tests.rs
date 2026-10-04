//! Native descriptor identity is portable, typed, bounded and fail closed.

use velnor_actions_contract::config::{NativeDesktopProfile, WorkloadConfig};
use velnor_actions_contract::{ProposedTask, canonical_json_bytes};

use super::{MAX_DESCRIPTOR_BYTES, NATIVE_DESKTOP_PROFILE_KEY, desktop_profile, profile_identity};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn profile() -> NativeDesktopProfile {
    serde_json::from_value(serde_json::json!({
        "ffi": {
            "manifest_path": "components/orbit-bridge/Cargo.toml",
            "package": "orbit-bridge", "profile": "native-release", "features": ["native"],
            "framework_name": "OrbitCore", "module_name": "OrbitCoreFFI",
            "static_library": "liborbit_bridge.a", "bindings_path": "generated/orbit-bindings",
            "xcframework_path": "build/OrbitCore.xcframework"
        },
        "native_root": "clients/orbit", "deployment_target": "25.3"
    }))
    .expect("closed second repository profile")
}

fn task(raw: Option<&str>) -> ProposedTask {
    let workload: WorkloadConfig = serde_json::from_value(serde_json::json!({
        "name": "orbit", "kind": "native_swift_package_ci", "root": "clients/orbit"
    }))
    .expect("closed workload");
    let mut task = super::super::proposal(&workload, "check", vec!["check".to_owned()], None);
    task.identity.project_root = ".".to_owned();
    if let Some(raw) = raw {
        task.identity
            .environment
            .insert(NATIVE_DESKTOP_PROFILE_KEY.to_owned(), raw.to_owned());
    }
    task
}

#[test]
fn second_repository_profile_identity_is_canonical_and_tracks_mutation() -> TestResult {
    let profile = profile();
    let compact = task(Some(&serde_json::to_string(&profile)?));
    let pretty = task(Some(&serde_json::to_string_pretty(&profile)?));
    assert_eq!(
        desktop_profile(&[&compact, &pretty])?,
        Some(profile.clone())
    );
    assert_eq!(profile_identity(&compact)?, profile_identity(&pretty)?);
    assert_eq!(
        profile_identity(&compact)?,
        Some(String::from_utf8(canonical_json_bytes(&profile)?)?)
    );
    let mut changed = profile;
    changed.ffi.package = "orbit-worker".to_owned();
    let changed = task(Some(&serde_json::to_string(&changed)?));
    assert_ne!(profile_identity(&compact)?, profile_identity(&changed)?);
    assert!(desktop_profile(&[&compact, &changed]).is_err());
    Ok(())
}

#[test]
fn every_native_phase_requires_profile_without_payload_or_null_fallback() -> TestResult {
    assert_eq!(desktop_profile(&[])?, None);
    for raw in [
        None,
        Some("null"),
        Some("{}"),
        Some("false"),
        Some("{"),
        Some("[]"),
    ] {
        let mut task = task(raw);
        task.payload = vec![serde_json::to_string(&profile())?.into()];
        assert!(desktop_profile(&[&task]).is_err());
        assert!(profile_identity(&task).is_err());
    }
    let present = task(Some(&serde_json::to_string(&profile())?));
    let absent = task(None);
    for pair in [[&present, &absent], [&absent, &present]] {
        assert!(desktop_profile(&pair).is_err());
    }
    Ok(())
}

#[test]
fn unrelated_workloads_without_native_metadata_have_no_profile_identity() -> TestResult {
    let mut other = task(None);
    other.configuration = "node_ci".to_owned();
    assert_eq!(desktop_profile(&[&other])?, None);
    assert_eq!(profile_identity(&other)?, None);
    other
        .identity
        .environment
        .insert(NATIVE_DESKTOP_PROFILE_KEY.to_owned(), "null".to_owned());
    assert!(desktop_profile(&[&other]).is_err());
    Ok(())
}

#[test]
fn all_source_identity_fields_bind_group_ownership() -> TestResult {
    let first = task(Some(&serde_json::to_string(&profile())?));
    for field in [
        "stack",
        "configuration",
        "component",
        "unit_id",
        "unit_key",
        "unit_path",
        "root",
    ] {
        let mut other = first.clone();
        match field {
            "stack" => other.stack_id = "rust".to_owned(),
            "configuration" => other.configuration = "native_xcode_project_ci".to_owned(),
            "component" => other.component_id = "other".to_owned(),
            "unit_id" => other.identity.unit_id = "workload:other".to_owned(),
            "unit_key" => other.identity.unit_key = "other".to_owned(),
            "unit_path" => other.identity.unit_path = "clients/other".to_owned(),
            _ => other.identity.project_root = "clients/orbit".to_owned(),
        }
        let error = desktop_profile(&[&first, &other]).expect_err("mixed native owner");
        assert!(
            error.to_string().contains("mixed_group_identity"),
            "{field}: {error}"
        );
    }
    Ok(())
}

#[test]
fn profile_json_rejects_unknown_fields_and_unsafe_typed_values() -> TestResult {
    let mut unknown = serde_json::to_value(profile())?;
    unknown["run"] = serde_json::json!("arbitrary script");
    assert!(profile_identity(&task(Some(&unknown.to_string()))).is_err());
    let mut nested = serde_json::to_value(profile())?;
    nested["ffi"]["args"] = serde_json::json!(["--unreviewed"]);
    assert!(profile_identity(&task(Some(&nested.to_string()))).is_err());
    for field in ["native_root", "deployment_target"] {
        let mut invalid = serde_json::to_value(profile())?;
        invalid[field] = serde_json::json!("../escape");
        assert!(profile_identity(&task(Some(&invalid.to_string()))).is_err());
    }
    let mut invalid = profile();
    invalid.ffi.features = vec!["z".to_owned(), "a".to_owned()];
    assert!(profile_identity(&task(Some(&serde_json::to_string(&invalid)?))).is_err());
    Ok(())
}

#[test]
fn single_task_requires_valid_owner_checkout_root_and_native_configuration() -> TestResult {
    let original = task(Some(&serde_json::to_string(&profile())?));
    for configuration in [
        "node_ci",
        "jackin_swift_package_ci",
        "native_xcode_project_ci",
    ] {
        let mut invalid = original.clone();
        invalid.configuration = configuration.to_owned();
        assert!(profile_identity(&invalid).is_err());
    }
    let mut invalid = original.clone();
    invalid.identity.project_root = "clients/orbit".to_owned();
    assert!(profile_identity(&invalid).is_err());
    let mut invalid = original.clone();
    invalid.identity.unit_path = "../orbit".to_owned();
    assert!(profile_identity(&invalid).is_err());
    let mut invalid = original.clone();
    invalid.identity.unit_id = "workload:other".to_owned();
    assert!(profile_identity(&invalid).is_err());
    let mut invalid = original;
    invalid.stack_id = "rust".to_owned();
    assert!(profile_identity(&invalid).is_err());
    Ok(())
}

#[test]
fn descriptor_bytes_are_bounded_before_decoding() {
    let raw = " ".repeat(MAX_DESCRIPTOR_BYTES + 1);
    let error = profile_identity(&task(Some(&raw))).expect_err("bounded descriptor");
    assert!(error.to_string().contains("descriptor_size"));
}

#[test]
fn xcode_profile_binds_second_repository_app_and_kind() -> TestResult {
    let mut profile = profile();
    profile.apple = Some(serde_json::from_value(serde_json::json!({
        "project_spec": "native.yaml", "project_path": "Orbit.xcodeproj",
        "scheme": "OrbitApp", "app_name": "OrbitApp",
        "bundle_identifier": "org.example.orbit", "bundle_name": "Orbit Desktop",
        "app_path": "build/OrbitApp.app", "derived_data_path": "build/orbit-data",
        "archive_name_prefix": "orbit-desktop"
    }))?);
    let mut xcode = task(Some(&serde_json::to_string(&profile)?));
    xcode.configuration = "native_xcode_project_ci".to_owned();
    assert_eq!(desktop_profile(&[&xcode])?, Some(profile.clone()));
    profile.apple.as_mut().expect("app profile").scheme = "OrbitOther".to_owned();
    let mut changed = task(Some(&serde_json::to_string(&profile)?));
    changed.configuration = "native_xcode_project_ci".to_owned();
    assert_ne!(profile_identity(&xcode)?, profile_identity(&changed)?);
    assert!(desktop_profile(&[&xcode, &changed]).is_err());
    let swift = task(Some(&serde_json::to_string(&profile)?));
    assert!(profile_identity(&swift).is_err());
    Ok(())
}
