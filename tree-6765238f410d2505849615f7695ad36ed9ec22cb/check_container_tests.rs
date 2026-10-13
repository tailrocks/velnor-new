//! Unqualified or mutable host executable selection never reaches a probe.
use super::*;

fn deadline() -> CheckDeadline {
    CheckDeadline::after(std::time::Duration::from_secs(60)).expect("deadline")
}

#[test]
fn wrong_cli_digest_never_executes_or_materializes() {
    let temp = tempfile::TempDir::new().expect("temp");
    let root = temp.path().canonicalize().expect("canonical temp");
    let source = root.join("docker");
    let destination = root.join("owned");
    let marker = root.join("invoked");
    std::fs::write(
        &source,
        format!("#!/bin/sh\nprintf yes > '{}'\n", marker.display()),
    )
    .expect("source");
    assert!(project_executable(&source, &destination, &"0".repeat(64), deadline()).is_err());
    assert!(!destination.exists());
    assert!(!marker.exists());
}

#[test]
fn exact_bytes_remain_owned_after_host_replacement() {
    let temp = tempfile::TempDir::new().expect("temp");
    let root = temp.path().canonicalize().expect("canonical temp");
    let source = root.join("docker");
    let destination = root.join("owned");
    let bytes = b"qualified docker fixture";
    let digest = crate::cover_identity::generator::sha256_hex(bytes);
    std::fs::write(&source, bytes).expect("source");
    assert_eq!(
        project_executable(&source, &destination, &digest, deadline()).expect("copy"),
        digest
    );
    std::fs::write(&source, b"replacement").expect("replace");
    assert_eq!(std::fs::read(&destination).expect("owned"), bytes);
    assert!(project_executable(&source, &destination, &digest, deadline()).is_err());
}

#[cfg(unix)]
#[test]
fn host_symlink_selection_is_rejected() {
    let temp = tempfile::TempDir::new().expect("temp");
    let root = temp.path().canonicalize().expect("canonical temp");
    let source = root.join("docker");
    let link = root.join("linked");
    std::fs::write(&source, b"fixture").expect("source");
    std::os::unix::fs::symlink(&source, &link).expect("link");
    assert!(project_executable(&link, &root.join("owned"), &"0".repeat(64), deadline()).is_err());
}

#[test]
fn owned_cli_special_permission_bits_are_rejected() {
    let temp = tempfile::TempDir::new().expect("temp");
    let home = temp.path().canonicalize().expect("home");
    std::fs::create_dir(home.join("bin")).expect("bin");
    let source = home.join("source");
    let bytes = b"qualified docker fixture";
    let sha256 = crate::cover_identity::generator::sha256_hex(bytes);
    std::fs::write(&source, bytes).expect("source");
    let docker_program = home.join("bin/docker");
    project_executable(&source, &docker_program, &sha256, deadline()).expect("copy");
    let prepared = PreparedContainer {
        home: home.clone(),
        docker_config: home.join("docker"),
        docker_program: docker_program.clone(),
        docker_sha256: sha256,
        orbctl_program: None,
        orbctl_sha256: None,
        endpoint: "unix:///qualified/docker.sock".into(),
    };
    verify_owned_cli(&prepared, Some(deadline())).expect("ordinary owned mode");
    for mode in [0o4_500, 0o2_500, 0o1_500] {
        std::fs::set_permissions(&docker_program, std::fs::Permissions::from_mode(mode))
            .expect("mutate mode");
        assert!(
            verify_owned_cli(&prepared, Some(deadline())).is_err(),
            "{mode:o}"
        );
    }
}
