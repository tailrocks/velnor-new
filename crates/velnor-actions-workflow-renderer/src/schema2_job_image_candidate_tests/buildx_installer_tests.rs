use super::*;

#[test]
fn buildx_installer_uses_the_lock_and_rejects_a_wrong_checksum() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    const PAYLOAD: &str = "buildx-fixture";
    const SHA256: &str = "21d63ac724e14102a2f8bc4ed01b0394c02bf09d44513ba1ed3b4a9b77362d83";
    let root = temporary_directory("buildx-installer");
    let workspace = root.join("workspace");
    let runner_temp = root.join("runner-temp");
    let fake_bin = root.join("bin");
    fs::create_dir_all(workspace.join(".velnor")).expect("The source lock path is not available.");
    fs::create_dir_all(&fake_bin).expect("The fake command path is not available.");
    fs::write(workspace.join(".velnor/runner.lock"), lock_text(SHA256))
        .expect("The source lock file is not available.");
    let fake_curl = fake_bin.join("curl");
    fs::write(
        &fake_curl,
        format!(
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = --output ]; then\n    shift\n    printf '%s' '{PAYLOAD}' > \"$1\"\n    exit 0\n  fi\n  shift\ndone\nexit 2\n"
        ),
    )
    .expect("Write the curl test fixture.");
    fs::set_permissions(&fake_curl, fs::Permissions::from_mode(0o755))
        .expect("Set the curl test fixture mode.");

    let success = run_installer(&workspace, &runner_temp, &fake_bin);
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    let plugin = runner_temp.join("velnor-docker-config/cli-plugins/docker-buildx");
    assert_eq!(
        fs::read(&plugin).expect("The installed plug-in is not available."),
        PAYLOAD.as_bytes()
    );
    assert_eq!(
        fs::metadata(&plugin)
            .expect("The plug-in metadata is not available.")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        fs::metadata(
            plugin
                .parent()
                .expect("The plug-in directory is not available.")
        )
        .expect("The plug-in directory metadata is not available.")
        .permissions()
        .mode()
            & 0o777,
        0o700
    );

    let wrong_temp = root.join("wrong-runner-temp");
    fs::write(
        workspace.join(".velnor/runner.lock"),
        lock_text(&"0".repeat(64)),
    )
    .expect("Write a source checksum with an incorrect value.");
    let failure = run_installer(&workspace, &wrong_temp, &fake_bin);
    assert!(!failure.status.success());
    assert!(
        String::from_utf8_lossy(&failure.stderr)
            .contains("The Buildx checksum is different from the value in .velnor/runner.lock.")
    );
    assert!(
        !wrong_temp
            .join("velnor-docker-config/cli-plugins/docker-buildx")
            .exists()
    );
    fs::remove_dir_all(root).expect("Remove the installer test files.");
}
