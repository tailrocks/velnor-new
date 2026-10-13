//! Process-level verification against the checked-in historical Sigstore bundle.

use std::error::Error;
use std::fs;
use std::path::Path;

use base64::Engine;
use serde_json::{Value, json};
#[path = "native_helper_protocol_support/mod.rs"]
mod native_helper_protocol_support;
use native_helper_protocol_support::{
    assert_private_directory, assert_private_executable, assert_read_only_install,
    restore_install_writability, set_private_directory, set_read_only_executable,
    set_read_only_install,
};

const HISTORICAL_SOURCE: &str = "4f6def90e7b1008626db18675d1cac129b8f2ad7";
const HISTORICAL_WORKFLOW: &str =
    "https://github.com/tailrocks/velnor-new/.github/workflows/image-release.yml@refs/heads/main";
const HISTORICAL_SIGNER: &str =
    "https://github.com/tailrocks/velnor-new/.github/workflows/image-release.yml@refs/heads/main";

#[tokio::test]
async fn compiled_helper_verifies_real_bundle_and_rejects_claim_mutations()
-> Result<(), Box<dyn Error>> {
    #[cfg(unix)]
    let directory = {
        use std::os::unix::fs::PermissionsExt;

        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()?
    };
    #[cfg(not(unix))]
    let directory = tempfile::Builder::new().tempdir()?;
    let private_directory = directory.path().canonicalize()?;
    let install_directory = private_directory.join("installed-bin");
    fs::create_dir(&install_directory)?;
    let state_directory = private_directory.join("service-state");
    fs::create_dir(&state_directory)?;
    set_private_directory(&state_directory)?;
    let helper = install_directory.join("velnor-runner-attestation-helper");
    fs::copy(
        env!("CARGO_BIN_EXE_velnor-runner-attestation-helper"),
        &helper,
    )?;
    assert_private_directory(&private_directory)?;
    assert_private_directory(&state_directory)?;
    set_read_only_install(&install_directory)?;
    set_read_only_executable(&helper)?;
    assert_read_only_install(&install_directory)?;
    assert_private_executable(&helper)?;

    let request = historical_request(&state_directory)?;
    native_helper_protocol_support::verify_persistent_state(
        &helper,
        &install_directory,
        &state_directory,
        &request,
    )
    .await?;
    native_helper_protocol_support::reject_invalid_state_roots(
        &helper,
        &private_directory,
        &request,
    )
    .await?;
    native_helper_protocol_support::reject_invalid_requests(&helper, &private_directory, &request)
        .await?;
    restore_install_writability(&install_directory)?;
    Ok(())
}

fn historical_request(state_directory: &Path) -> Result<Value, Box<dyn Error>> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let bundle = fs::read(fixtures.join("api-SHA256SUMS-attestation-1.bundle.json"))?;
    let checksums = fs::read(fixtures.join("SHA256SUMS"))?;
    let state_directory = state_directory
        .to_str()
        .ok_or("service state path is not UTF-8")?;
    Ok(json!({
        "schema": 2,
        "state_directory": state_directory,
        "bundle_base64": base64::engine::general_purpose::STANDARD.encode(bundle),
        "checksum_base64": base64::engine::general_purpose::STANDARD.encode(checksums),
        "expected": {
            "signer": HISTORICAL_SIGNER,
            "signer_digest": HISTORICAL_SOURCE,
            "source": "https://github.com/tailrocks/velnor-new",
            "source_digest": HISTORICAL_SOURCE,
            "source_ref": "refs/heads/main",
            "build_config": HISTORICAL_WORKFLOW,
            "build_config_digest": HISTORICAL_SOURCE
        },
        "checksum_subject": {
            "name": "SHA256SUMS",
            "digest": "4fb1c0c54b92e84e68c816d621f5cb83c946db392995cd245fa6d61b0bd1e491"
        },
        "target_subject": {
            "name": "velnor-runner-linux-amd64.tar",
            "digest": "21b54ea99c42b3a932713ee3999064a6f2cedaf992be1c6125bbf92f07f5761b"
        }
    }))
}
