use super::{ASSET, Workspace, metadata_json, run_script, scripts, successful};
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn runtime_identity_accepts_virtual_workspace_member_and_rejects_boundary_mismatches() {
    let workspace = Workspace::new("identity");
    let identity = scripts::identity("cat \"$METADATA_FILE\"");
    let output = workspace.scratch.path().join("identity.out");
    let env = workspace.env(&output);
    let result = run_script(&identity, &env, None);
    assert!(
        successful(&result),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let values = fs::read_to_string(&output).expect("identity outputs");
    assert!(values.contains("package_version=0.4.3\n"));
    assert!(values.contains("tag=repo-scan-v0.4.3\n"));

    fs::write(
        &workspace.metadata,
        metadata_json(&workspace.root, &["gpu"], false),
    )
    .expect("required-feature metadata");
    let result = run_script(
        &identity,
        &workspace.env(&workspace.scratch.path().join("features.out")),
        None,
    );
    assert!(
        !successful(&result),
        "required-feature target must fail closed"
    );

    fs::write(
        &workspace.metadata,
        metadata_json(&workspace.root, &[], true),
    )
    .expect("outside package metadata");
    let result = run_script(
        &identity,
        &workspace.env(&workspace.scratch.path().join("outside.out")),
        None,
    );
    assert!(
        !successful(&result),
        "package outside workspace must fail closed"
    );

    fs::write(
        &workspace.metadata,
        metadata_json(&workspace.root, &[], false),
    )
    .expect("restore metadata");
    let mut wrong_bin = workspace.env(&workspace.scratch.path().join("wrong-bin.out"));
    wrong_bin.retain(|(key, _)| key != "BINARY_NAME");
    wrong_bin.push(("BINARY_NAME".to_owned(), "other".to_owned()));
    let result = run_script(&identity, &wrong_bin, None);
    assert!(!successful(&result), "binary mismatch must fail closed");
}

#[test]
fn build_uses_fresh_explicit_target_directory_for_virtual_workspace_member() {
    let workspace = Workspace::new("build");
    let file_stub = workspace.scratch.path().join("bin");
    fs::create_dir(&file_stub).expect("create command stubs");
    let file_command = file_stub.join("file");
    fs::write(
        &file_command,
        "#!/bin/sh\nprintf '%s\\n' 'Mach-O 64-bit executable arm64'\n",
    )
    .expect("write file stub");
    fs::set_permissions(&file_command, fs::Permissions::from_mode(0o755))
        .expect("make file stub executable");
    let identity = scripts::identity("cat \"$METADATA_FILE\"");
    let build = scripts::build(
        &identity,
        ":",
        "mkdir -p \"$CARGO_TARGET_DIR/$TARGET_TRIPLE/release\"; printf 'arm64-fixture\\n' > \"$CARGO_TARGET_DIR/$TARGET_TRIPLE/release/$BINARY_NAME\"",
    );
    let output = workspace.scratch.path().join("build.out");
    let mut env = workspace.env(&output);
    env.extend([
        (
            "RUNNER_TEMP".to_owned(),
            workspace.runner_temp.display().to_string(),
        ),
        ("GITHUB_RUN_ID".to_owned(), "17".to_owned()),
        ("GITHUB_RUN_ATTEMPT".to_owned(), "1".to_owned()),
        (
            "GITHUB_REPOSITORY".to_owned(),
            "example/repo-scan".to_owned(),
        ),
        ("GITHUB_SHA".to_owned(), workspace.source_sha.clone()),
        ("EXPECTED_VERSION".to_owned(), "0.4.3".to_owned()),
        ("EXPECTED_TAG".to_owned(), "repo-scan-v0.4.3".to_owned()),
        (
            "TARGET_TRIPLE".to_owned(),
            "aarch64-apple-darwin".to_owned(),
        ),
    ]);
    let result = run_script(&build, &env, Some(&file_stub));
    assert!(
        successful(&result),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let target_dir = workspace.runner_temp.join("velnor-binary-target-17-1");
    assert!(
        target_dir
            .join("aarch64-apple-darwin/release/repo-scan")
            .is_file()
    );
    assert!(!workspace.root.join("target").exists());
    let assets = workspace.runner_temp.join("consumer-binary-assets");
    assert!(assets.join(ASSET).is_file());
    assert!(assets.join("SHA256SUMS").is_file());
    assert!(
        fs::read_to_string(assets.join("release.json"))
            .expect("receipt")
            .contains(&workspace.source_sha)
    );
}
