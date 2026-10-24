//! Export and enforce the canonical preview source inventory for a Homebrew tap.

use std::path::Path;

use velnor_actions_contract::{
    config::HomebrewTapConfig, homebrew_authority::HOMEBREW_SOURCE_RELEASE_POLICY_PATH,
    package_release::PackageReleaseChannel, parse_package_release_policy,
};

use crate::{
    OrchestratorError,
    internal::internal,
    prepare::{GenerationPreparation, prepare},
    safe_read::{MAX_REPO_FILE_BYTES, RepoBytes, read_repo_bytes},
};

/// Derive and exclusively write the current preview source policy.
///
/// The destination must be absolute and new. The caller can then review and
/// commit this exact data file before enabling Homebrew source authority.
///
/// # Errors
/// Returns an error when the project lacks an enabled Homebrew preview, the
/// destination is not absolute or already exists, or preparation fails.
pub fn export_homebrew_source_policy(root: &Path, output: &Path) -> Result<(), OrchestratorError> {
    if !output.is_absolute() {
        return Err(internal("homebrew_source_policy_output_must_be_absolute"));
    }
    let prep = prepare(root)?;
    require_enabled_tap(&prep)?;
    let bytes = source_policy_bytes(&prep)?;
    crate::exclusive_write::write_exclusive(output, &bytes, "homebrew_source_policy")
}

/// Check the repository's committed preview source policy against discovery.
///
/// # Errors
/// Returns an error when the policy is missing, noncanonical, stale, or the
/// project does not enable the Homebrew preview source.
pub fn check_homebrew_source_policy(root: &Path) -> Result<(), OrchestratorError> {
    let prep = prepare(root)?;
    require_enabled_tap(&prep)?;
    check_preparation(&prep)
}

/// Whether generated source-policy enforcement applies to the selected tap route.
///
/// Native formula generation requires the committed source policy. A captured,
/// source-qualified custom updater retains its original compatibility contract.
pub(crate) fn requires_native_generation_policy(tap: Option<&HomebrewTapConfig>) -> bool {
    tap.is_some_and(|tap| tap.enabled && tap.updater.is_none())
}

/// Enforce this policy only for repositories that enable a Homebrew tap.
pub(crate) fn check_preparation(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    if !tap_enabled(prep) {
        return Ok(());
    }
    let expected = source_policy_bytes(prep)?;
    let observed = read_repo_bytes(
        &prep.root,
        HOMEBREW_SOURCE_RELEASE_POLICY_PATH,
        MAX_REPO_FILE_BYTES,
    )?;
    validate_policy_file(&expected, observed)
}

fn validate_policy_file(expected: &[u8], observed: RepoBytes) -> Result<(), OrchestratorError> {
    match observed {
        RepoBytes::Absent => Err(internal("homebrew_source_policy_missing")),
        RepoBytes::Bytes(bytes) => validate_policy_bytes(expected, &bytes),
    }
}

fn source_policy_bytes(prep: &GenerationPreparation) -> Result<Vec<u8>, OrchestratorError> {
    let release = prep
        .config
        .stacks
        .rust
        .as_ref()
        .map(|rust| &rust.binary_release)
        .filter(|release| release.preview.enabled)
        .ok_or_else(|| internal("homebrew_source_policy_preview_disabled"))?;
    let policy =
        crate::binary_preview_emit::release_policy(prep, release, PackageReleaseChannel::Preview)?;
    policy
        .canonical_bytes()
        .map_err(|error| internal(&error.to_string()))
}

fn require_enabled_tap(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    if !tap_enabled(prep) {
        return Err(internal("homebrew_source_policy_tap_disabled"));
    }
    Ok(())
}

fn tap_enabled(prep: &GenerationPreparation) -> bool {
    prep.config
        .distribution
        .homebrew_tap
        .as_ref()
        .is_some_and(|tap| tap.enabled)
}

fn validate_policy_bytes(expected: &[u8], observed: &[u8]) -> Result<(), OrchestratorError> {
    parse_package_release_policy(observed)
        .map_err(|_| internal("homebrew_source_policy_noncanonical"))?;
    if observed != expected {
        return Err(internal("homebrew_source_policy_stale"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_policy_bytes, validate_policy_file};
    use crate::safe_read::RepoBytes;
    use velnor_actions_contract::{
        CargoVersionBinding, CargoVersionPolicy, PACKAGE_RELEASE_POLICY_SCHEMA_V1,
        PackageReleasePlanProduct, PackageReleasePolicy,
        package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    };

    fn preview_policy_bytes() -> Vec<u8> {
        PackageReleasePolicy {
            schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
            channel: PackageReleaseChannel::Preview,
            repository: "example/project".to_owned(),
            default_branch: "main".to_owned(),
            version: CargoVersionPolicy {
                version_package: "jackin".to_owned(),
                bindings: vec![CargoVersionBinding {
                    package: "jackin".to_owned(),
                    manifest_path: "Cargo.toml".to_owned(),
                    workspace_manifest_path: "Cargo.toml".to_owned(),
                }],
            },
            products: vec![PackageReleasePlanProduct {
                id: "jackin".to_owned(),
                package: "jackin".to_owned(),
                bins: vec!["jackin".to_owned()],
                targets: vec!["aarch64-apple-darwin".to_owned()],
                format: PackageReleasePayloadFormat::TarGz,
            }],
            companions: Vec::new(),
        }
        .canonical_bytes()
        .expect("valid fixture policy")
    }

    #[test]
    fn only_native_generation_requires_the_new_committed_source_policy() {
        let native = crate::homebrew::test_support::policy();
        assert!(super::requires_native_generation_policy(Some(&native)));

        let mut custom = native;
        custom.updater = Some(velnor_actions_contract::config::TapUpdaterBinding {
            task: "tap:update".to_owned(),
            source: velnor_actions_contract::config::MiseTaskSource {
                mise_config: "mise.toml".to_owned(),
                working_directory: ".".to_owned(),
            },
            runner: velnor_actions_contract::config::CheckRunner {
                label: "ubuntu-26.04".to_owned(),
                platform: velnor_actions_contract::config::CheckPlatform::LinuxX64,
                executor: velnor_actions_contract::config::CheckExecutor::Hosted,
                container: None,
            },
            inputs: vec!["mise.toml".to_owned(), "Formula/cli-preview.rb".to_owned()],
            tools: vec!["ruby".to_owned()],
            timeout_minutes: 5,
        });
        assert!(!super::requires_native_generation_policy(Some(&custom)));
        assert!(!super::requires_native_generation_policy(None));
    }

    #[test]
    fn accepts_only_exact_canonical_policy_bytes() {
        let expected = preview_policy_bytes();
        assert!(validate_policy_bytes(&expected, &expected).is_ok());

        let changed = expected
            .windows(b"jackin".len())
            .enumerate()
            .filter_map(|(index, window)| (window == b"jackin").then_some(index))
            .fold(expected.clone(), |mut bytes, index| {
                bytes[index..index + b"jackin".len()].copy_from_slice(b"jackix");
                bytes
            });
        assert!(validate_policy_bytes(&expected, &changed).is_err());

        let mut noncanonical = expected.clone();
        noncanonical.insert(0, b' ');
        assert!(validate_policy_bytes(&expected, &noncanonical).is_err());
        assert!(validate_policy_file(&expected, RepoBytes::Absent).is_err());
    }
}
