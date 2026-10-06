//! Source-bound Swift helper closure and typed profile support records.

use crate::support::{OwnedSupportFile, SupportBundle};
use velnor_actions_contract::{
    ContractError,
    config::{NativeDesktopProfile, SwiftInputs, is_valid_workload_name},
};

/// Generator-owned native dispatcher.
pub const DESKTOP_NATIVE_ENTRY: &str = ".github/velnor/desktop/desktop_native.py";
/// Serialized native profile used for delivery.
pub const DESKTOP_RELEASE_PROFILE: &str = ".github/velnor/desktop/desktop_release_profile.json";
/// Serialized native profile used for verification cadences.
pub const DESKTOP_VERIFICATION_PROFILE: &str =
    ".github/velnor/desktop/desktop_verification_profile.json";

const HELPERS: [(&str, &str); 6] = [
    (DESKTOP_NATIVE_ENTRY, include_str!("desktop_native.py")),
    (
        ".github/velnor/desktop/desktop_native_core.py",
        include_str!("desktop_native_core.py"),
    ),
    (
        ".github/velnor/desktop/desktop_native_build.py",
        include_str!("desktop_native_build.py"),
    ),
    (
        ".github/velnor/desktop/desktop_native_verify.py",
        include_str!("desktop_native_verify.py"),
    ),
    (
        ".github/velnor/desktop/desktop_native_sign.py",
        include_str!("desktop_native_sign.py"),
    ),
    (
        ".github/velnor/desktop/desktop_native_state.py",
        include_str!("desktop_native_state.py"),
    ),
];

/// Exact static companion helper paths for generator ownership checks.
#[must_use]
pub fn desktop_helper_paths() -> Vec<&'static str> {
    HELPERS.iter().map(|(path, _)| *path).collect()
}

/// Static ownership inventory, including both delivery and verification profiles.
#[must_use]
pub fn desktop_owned_paths() -> Vec<&'static str> {
    let mut paths = desktop_helper_paths();
    paths.extend([DESKTOP_RELEASE_PROFILE, DESKTOP_VERIFICATION_PROFILE]);
    paths
}

/// Emit the complete compiled helper closure with standard source markers.
/// # Errors
/// Rejects malformed fixed source or generator version markers.
pub fn support_sources(version: &str) -> Result<SupportBundle, ContractError> {
    let files = HELPERS
        .iter()
        .map(|(path, body)| OwnedSupportFile::compiled(path, body, version))
        .collect::<Result<Vec<_>, _>>()?;
    SupportBundle::compiled(files)
}

/// Emit the validated Swift consumer input projection with its source marker.
/// # Errors
/// Rejects invalid profiles, unmanaged profile destinations, or malformed markers.
pub fn profile_file(
    path: &str,
    profile: &NativeDesktopProfile,
    version: &str,
) -> Result<OwnedSupportFile, ContractError> {
    profile.validate(".velnor/config.toml", "desktop.profile")?;
    projected_profile_file(path, &SwiftInputs::from(profile), version)
}

/// Emit validated native consumer inputs through the canonical profile serializer.
/// # Errors
/// Rejects invalid native inputs, unmanaged destinations, or malformed markers.
pub fn projected_profile_file(
    path: &str,
    inputs: &SwiftInputs,
    version: &str,
) -> Result<OwnedSupportFile, ContractError> {
    inputs.validate(".velnor/config.toml", "desktop.profile")?;
    if !valid_profile_path(path) {
        return Err(ContractError::identity(
            "native_support",
            "invalid_desktop_profile_path",
        ));
    }
    let json = serde_json::to_string_pretty(inputs)
        .map_err(|error| ContractError::identity("native_support", error.to_string()))?;
    OwnedSupportFile::compiled(path, &format!("{json}\n"), version)
}

/// Accept only generator-owned delivery, verification, or named workload profiles.
#[must_use]
pub fn valid_profile_path(path: &str) -> bool {
    if [DESKTOP_RELEASE_PROFILE, DESKTOP_VERIFICATION_PROFILE].contains(&path) {
        return true;
    }
    path.strip_prefix(".github/velnor/desktop/workloads/")
        .and_then(|name| name.strip_suffix(".json"))
        .is_some_and(is_valid_workload_name)
}

#[cfg(test)]
mod tests {
    use super::{
        DESKTOP_RELEASE_PROFILE, desktop_owned_paths, support_sources, valid_profile_path,
    };

    #[test]
    fn complete_marked_helpers() -> Result<(), Box<dyn std::error::Error>> {
        let bundle = support_sources("1.2.3")?;
        assert_eq!(bundle.files().len(), 6);
        assert_eq!(desktop_owned_paths().len(), 8);
        for file in bundle.files() {
            assert!(file.path().starts_with(".github/velnor/desktop/"));
            assert!(file.source().contains("1.2.3"));
        }
        Ok(())
    }

    #[test]
    fn profile_paths_match_workload_name_domain() {
        assert!(valid_profile_path(DESKTOP_RELEASE_PROFILE));
        assert!(valid_profile_path(
            ".github/velnor/desktop/workloads/orbit.native.json"
        ));
        for path in [
            ".github/velnor/desktop/workloads/../escape.json",
            ".github/velnor/desktop/workloads/-option.json",
            ".github/velnor/desktop/workloads/Upper.json",
            ".github/velnor/desktop/workloads/.json",
            ".github/velnor/desktop/unknown.json",
        ] {
            assert!(!valid_profile_path(path));
        }
    }

    #[test]
    fn projected_profile_serialization_is_canonical() -> Result<(), Box<dyn std::error::Error>> {
        let mut inputs: velnor_actions_contract::config::SwiftInputs = serde_json::from_value(
            serde_json::json!({
                "ffi": {"bindings_path":"generated/Orbit", "xcframework_path":"build/Orbit.xcframework",
                    "framework_name":"Orbit", "module_name":"OrbitFFI", "static_library":"liborbit.a"},
                "native_root":".", "target":"apple-arm64", "deployment_target":"26.0"
            }),
        )?;
        let file = super::projected_profile_file(DESKTOP_RELEASE_PROFILE, &inputs, "1.2.3")?;
        assert_eq!(
            file,
            super::projected_profile_file(DESKTOP_RELEASE_PROFILE, &inputs, "1.2.3")?
        );
        assert!(!file.source().contains("manifest_path"));
        inputs.ffi.bindings_path = "../escape".to_owned();
        assert!(super::projected_profile_file(DESKTOP_RELEASE_PROFILE, &inputs, "1.2.3").is_err());
        Ok(())
    }
}
