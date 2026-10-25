use std::{fs, os::unix::fs::PermissionsExt};

use super::build::support::{install_verified_buildx, private_subdirectory};

const FIXTURE_BYTES: &[u8] = b"buildx-fixture";
const FIXTURE_SHA256: &str = "21d63ac724e14102a2f8bc4ed01b0394c02bf09d44513ba1ed3b4a9b77362d83";

#[test]
fn buildx_installer_verifies_lock_checksum_and_installs_mode_0755() {
    let temporary = tempfile::tempdir().expect("temp root");
    let private = private_subdirectory(temporary.path(), "docker-config/cli-plugins")
        .expect("private plugin directories");
    let stage = temporary.path().join("buildx.download");
    fs::write(&stage, FIXTURE_BYTES).expect("fixture bytes");
    let plugin = private.join("docker-buildx");

    install_verified_buildx(&stage, &plugin, FIXTURE_SHA256).expect("verified install");

    assert_eq!(fs::read(&plugin).expect("installed bytes"), FIXTURE_BYTES);
    assert_eq!(
        fs::metadata(&plugin)
            .expect("plugin metadata")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        fs::metadata(&private)
            .expect("plugin dir")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn buildx_installer_rejects_wrong_checksum_before_publication() {
    let temporary = tempfile::tempdir().expect("temp root");
    let private = private_subdirectory(temporary.path(), "docker-config/cli-plugins")
        .expect("private plugin directories");
    let stage = temporary.path().join("buildx.download");
    fs::write(&stage, FIXTURE_BYTES).expect("fixture bytes");
    let plugin = private.join("docker-buildx");

    let error = install_verified_buildx(&stage, &plugin, &"0".repeat(64))
        .expect_err("reject wrong runner lock checksum");
    assert!(error.to_string().contains(".velnor/runner.lock"));
    assert!(!plugin.exists());
}
