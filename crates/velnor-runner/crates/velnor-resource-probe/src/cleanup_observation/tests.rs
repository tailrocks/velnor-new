use std::{
    fs,
    os::unix::fs::symlink,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    CleanupObservationRequest, lower_hex, observe_absent, open_cgroup_root, validate_cgroup_leaf,
    validate_request,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("velnor-cleanup-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _result = fs::remove_dir_all(&self.0);
    }
}

fn request() -> CleanupObservationRequest {
    CleanupObservationRequest {
        schema_version: 1,
        operation_id: "a".repeat(32),
        expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        cgroup_paths: ["/docker/runner-id".to_owned(), "/docker/dind-id".to_owned()],
    }
}

#[test]
fn request_requires_two_distinct_canonical_worker_paths() {
    assert!(validate_request(&request()).is_ok());
    let mut invalid = request();
    invalid.cgroup_paths[1] = invalid.cgroup_paths[0].clone();
    assert!(validate_request(&invalid).is_err());
    for path in ["/", "/docker/../host", "/docker//runner", "relative"] {
        assert!(validate_cgroup_leaf(path).is_err());
    }
    assert!(lower_hex(&"b".repeat(32)));
    assert!(!lower_hex(&"B".repeat(32)));
}

#[test]
fn exact_cgroup_leaf_must_be_absent() -> Result<(), String> {
    let scratch = Scratch::new()?;
    fs::create_dir_all(scratch.0.join("docker/runner-id")).map_err(|error| error.to_string())?;
    let root = open_cgroup_root(&scratch.0).map_err(|error| format!("open root: {error:?}"))?;

    assert!(observe_absent(&root, "/docker/dind-id").is_ok());
    assert!(observe_absent(&root, "/docker/runner-id").is_err());
    Ok(())
}

#[test]
fn symlink_components_fail_closed() -> Result<(), String> {
    let scratch = Scratch::new()?;
    fs::create_dir(scratch.0.join("real")).map_err(|error| error.to_string())?;
    symlink("real", scratch.0.join("alias")).map_err(|error| error.to_string())?;
    let root = open_cgroup_root(&scratch.0).map_err(|error| format!("open root: {error:?}"))?;
    assert!(observe_absent(&root, "/alias/worker").is_err());
    Ok(())
}

#[test]
fn cgroup_root_open_does_not_follow_a_symlink() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let target = scratch.0.join("target");
    fs::create_dir(&target).map_err(|error| error.to_string())?;
    let alias = scratch.0.join("alias");
    symlink(&target, &alias).map_err(|error| error.to_string())?;
    assert!(open_cgroup_root(&alias).is_err());
    assert!(open_cgroup_root(&alias.join("cgroup")).is_err());
    Ok(())
}
