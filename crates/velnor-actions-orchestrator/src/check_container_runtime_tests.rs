#[cfg(unix)]
mod unix_tests {
    use super::super::*;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use velnor_actions_contract::config::HostContainerProfile;

    fn docker_profile(socket: &Path, socket_uid: u32) -> HostContainerProfile {
        serde_json::from_value(json!({
            "provider": "docker",
            "context": "ci",
            "socket_path": socket.display().to_string(),
            "socket_uid": socket_uid,
            "cli": {
                "path": "/usr/bin/docker",
                "sha256": "0".repeat(64),
                "version": "27.0.0",
                "build": "fixture"
            },
            "daemon": {
                "version": "27.0.0",
                "platform": "linux_x64",
                "operating_system": "Docker fixture",
                "identity_policy": "execution_scoped"
            }
        }))
        .expect("docker profile")
    }

    fn orb_profile(socket: &Path, runtime: &Path, uid: u32) -> HostContainerProfile {
        serde_json::from_value(json!({
            "provider": "orb_stack",
            "context": "ci",
            "socket_path": socket.display().to_string(),
            "cli": {
                "path": "/usr/bin/docker",
                "sha256": "0".repeat(64),
                "version": "27.0.0",
                "build": "fixture"
            },
            "daemon": {
                "version": "27.0.0",
                "platform": "linux_x64",
                "operating_system": "OrbStack fixture",
                "identity_policy": "execution_scoped"
            },
            "sdk": {
                "app_bundle_path": "/Applications/OrbStack.app",
                "bundle_id": "com.orbstack.OrbStack",
                "team_id": "TEAM123456",
                "version": "1.0.0",
                "build": "1",
                "info_plist_sha256": "0".repeat(64),
                "main_executable_path": "Contents/MacOS/OrbStack",
                "main_executable_sha256": "0".repeat(64),
                "cli_bundle_path": "/Applications/OrbStack.app/Contents/CLI.app",
                "source_tree_sha256": "0".repeat(64),
                "owned_tree_sha256": "0".repeat(64),
                "cli_relative_path": "Contents/MacOS/orbctl",
                "cli_sha256": "0".repeat(64),
                "cli_version": "1.0.0",
                "cli_build": "1",
                "cli_commit": "0".repeat(40),
                "runtime_dir": runtime.display().to_string(),
                "runtime_uid": uid
            }
        }))
        .expect("orb profile")
    }

    fn socket(path: &Path) -> UnixListener {
        UnixListener::bind(path).expect("unix socket")
    }

    fn home_uid(path: &Path) -> u32 {
        fs::metadata(path).expect("home metadata").uid()
    }

    fn orb_uid(runtime: &Path, socket: &Path) -> u32 {
        let uid = home_uid(runtime);
        if uid > 0 {
            return uid;
        }
        let uid = 501;
        for path in [
            runtime,
            &runtime.join("status"),
            &runtime.join("vmgr.version"),
            socket,
        ] {
            set_fixture_owner(path, uid);
        }
        uid
    }

    fn set_fixture_owner(path: &Path, uid: u32) {
        rustix::fs::chownat(
            rustix::fs::CWD,
            path,
            Some(rustix::fs::Uid::from_raw(uid)),
            None,
            rustix::fs::AtFlags::empty(),
        )
        .expect("fixture owner");
    }

    #[test]
    fn docker_projection_is_credential_free_and_revalidates_exact_tree() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let listener = socket(&home.join("docker.sock"));
        let profile = docker_profile(&home.join("docker.sock"), home_uid(&home));
        let projection = prepare_runtime(&home, &profile).expect("projection");
        assert_eq!(
            projection.endpoint,
            format!("unix://{}", home.join("docker.sock").display())
        );
        let config: serde_json::Value = serde_json::from_slice(
            &fs::read(projection.docker_config.join("config.json")).expect("config"),
        )
        .expect("config json");
        assert_eq!(config["currentContext"], "ci");
        assert!(config.get("auths").is_none());
        let observed = observe_runtime(&projection).expect("runtime evidence");
        assert_eq!(observed.socket, projection.socket);
        assert_eq!(observed.runtime_root, projection.runtime_root);
        revalidate_runtime(&projection).expect("revalidate");
        drop(listener);
    }

    #[test]
    fn docker_revalidation_rejects_injected_context_files() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let listener = socket(&home.join("docker.sock"));
        let profile = docker_profile(&home.join("docker.sock"), home_uid(&home));
        let projection = prepare_runtime(&home, &profile).expect("projection");
        fs::write(projection.docker_config.join("credentials.json"), b"secret")
            .expect("injected file");
        assert!(revalidate_runtime(&projection).is_err());
        drop(listener);
    }

    #[test]
    fn socket_replacement_fails_continuity_check() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let path = home.join("docker.sock");
        let listener = socket(&path);
        let profile = docker_profile(&path, home_uid(&home));
        let projection = prepare_runtime(&home, &profile).expect("projection");
        drop(listener);
        fs::remove_file(&path).expect("old socket");
        let replacement = socket(&path);
        assert!(revalidate_runtime(&projection).is_err());
        drop(replacement);
    }

    #[test]
    fn declared_socket_owner_is_required() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let path = home.join("docker.sock");
        let listener = socket(&path);
        let wrong_uid = home_uid(&home).saturating_add(1);
        let profile = docker_profile(&path, wrong_uid);
        assert!(prepare_runtime(&home, &profile).is_err());
        drop(listener);
    }

    #[test]
    fn orb_rejects_long_owned_socket_path_before_runtime_link() {
        let temp = tempfile::TempDir::new().expect("temp");
        let root = temp.path().canonicalize().expect("canonical root");
        let home = root.join("h".repeat(120));
        fs::create_dir(&home).expect("long canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let runtime = root.join("runtime");
        fs::create_dir(&runtime).expect("runtime");
        fs::create_dir(runtime.join("status")).expect("status");
        fs::write(runtime.join("vmgr.version"), b"1").expect("version");
        let path = runtime.join("docker.sock");
        let listener = socket(&path);
        let profile = orb_profile(&path, &runtime, orb_uid(&runtime, &path));
        let error = prepare_runtime(&home, &profile).expect_err("owned socket must fit ABI");
        assert!(
            error.to_string().contains("runtime_socket_path_too_long"),
            "unexpected error: {error}"
        );
        assert!(!home.join(".orbstack").exists());
        assert_eq!(
            fs::read_dir(home.join("docker")).expect("config").count(),
            0
        );
        drop(listener);
    }

    #[test]
    fn orb_rejects_root_runtime_owner_independently_of_fixture_uid() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let runtime = home.join("runtime");
        fs::create_dir(&runtime).expect("runtime");
        fs::create_dir(runtime.join("status")).expect("status");
        fs::write(runtime.join("vmgr.version"), b"1").expect("version");
        let path = runtime.join("docker.sock");
        let listener = socket(&path);
        let profile = orb_profile(&path, &runtime, 0);
        let error = prepare_runtime(&home, &profile).expect_err("root owner is unsupported");
        assert!(error.to_string().contains("orbstack_runtime_owner"));
        drop(listener);
    }

    #[test]
    fn orb_projection_records_authority_and_owned_link() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let runtime = home.join("runtime");
        fs::create_dir(&runtime).expect("runtime");
        fs::create_dir(runtime.join("status")).expect("status");
        fs::write(runtime.join("vmgr.version"), b"1").expect("version");
        let path = runtime.join("docker.sock");
        let listener = socket(&path);
        let profile = orb_profile(&path, &runtime, orb_uid(&runtime, &path));
        let projection = prepare_runtime(&home, &profile).expect("projection");
        assert_eq!(projection.runtime_dir.as_deref(), Some(runtime.as_path()));
        assert_eq!(
            fs::read_link(projection.runtime_link.as_ref().expect("link")).expect("link target"),
            runtime
        );
        assert!(
            projection
                .runtime_entries
                .iter()
                .any(|entry| entry.path == "status")
        );
        assert!(
            projection
                .runtime_entries
                .iter()
                .any(|entry| entry.path == "vmgr.version")
        );
        assert!(
            projection
                .runtime_entries
                .iter()
                .any(|entry| entry.path == "docker.sock")
        );
        revalidate_runtime(&projection).expect("revalidate");
        drop(listener);
    }

    #[test]
    fn orb_runtime_workload_entries_may_change() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let runtime = home.join("runtime");
        fs::create_dir(&runtime).expect("runtime");
        fs::create_dir(runtime.join("status")).expect("status");
        fs::write(runtime.join("vmgr.version"), b"1").expect("version");
        let path = runtime.join("docker.sock");
        let listener = socket(&path);
        let profile = orb_profile(&path, &runtime, orb_uid(&runtime, &path));
        let projection = prepare_runtime(&home, &profile).expect("projection");
        fs::write(runtime.join("status/workload"), b"mutable").expect("workload");
        let runtime_uid = match &profile {
            HostContainerProfile::OrbStack { sdk, .. } => sdk.runtime_uid,
            HostContainerProfile::Docker { .. } => unreachable!("fixture profile is OrbStack"),
        };
        set_fixture_owner(&runtime.join("status/workload"), runtime_uid);
        revalidate_runtime(&projection).expect("mutable runtime remains admitted");
        drop(listener);
    }

    #[test]
    fn orb_runtime_root_mode_is_bound_and_not_writable() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let runtime = home.join("runtime");
        fs::create_dir(&runtime).expect("runtime");
        fs::create_dir(runtime.join("status")).expect("status");
        fs::write(runtime.join("vmgr.version"), b"1").expect("version");
        let original = fs::metadata(&runtime)
            .expect("runtime metadata")
            .permissions()
            .mode();
        let mut unsafe_permissions = fs::metadata(&runtime)
            .expect("runtime metadata")
            .permissions();
        unsafe_permissions.set_mode(original | 0o002);
        fs::set_permissions(&runtime, unsafe_permissions).expect("unsafe mode");
        let path = runtime.join("docker.sock");
        let listener = socket(&path);
        let profile = orb_profile(&path, &runtime, orb_uid(&runtime, &path));
        assert!(prepare_runtime(&home, &profile).is_err());
        let mut restored = fs::metadata(&runtime)
            .expect("runtime metadata")
            .permissions();
        restored.set_mode(original);
        fs::set_permissions(&runtime, restored).expect("restore mode");
        drop(listener);
    }

    #[test]
    fn orb_runtime_root_mode_change_fails_continuity() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let runtime = home.join("runtime");
        fs::create_dir(&runtime).expect("runtime");
        fs::create_dir(runtime.join("status")).expect("status");
        fs::write(runtime.join("vmgr.version"), b"1").expect("version");
        let path = runtime.join("docker.sock");
        let listener = socket(&path);
        let profile = orb_profile(&path, &runtime, orb_uid(&runtime, &path));
        let projection = prepare_runtime(&home, &profile).expect("projection");
        let original = fs::metadata(&runtime)
            .expect("runtime metadata")
            .permissions()
            .mode();
        let mut changed = fs::metadata(&runtime)
            .expect("runtime metadata")
            .permissions();
        changed.set_mode(original ^ 0o100);
        fs::set_permissions(&runtime, changed).expect("changed mode");
        assert!(revalidate_runtime(&projection).is_err());
        let mut restored = fs::metadata(&runtime)
            .expect("runtime metadata")
            .permissions();
        restored.set_mode(original);
        fs::set_permissions(&runtime, restored).expect("restore mode");
        drop(listener);
    }

    #[test]
    fn orb_runtime_symlinks_are_rejected_without_reading_targets() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let runtime = home.join("runtime");
        fs::create_dir(&runtime).expect("runtime");
        fs::create_dir(runtime.join("status")).expect("status");
        fs::write(runtime.join("vmgr.version"), b"1").expect("version");
        let path = runtime.join("docker.sock");
        let listener = socket(&path);
        let target = home.join("credentials");
        fs::write(&target, b"secret").expect("credential fixture");
        std::os::unix::fs::symlink(&target, runtime.join("credentials")).expect("runtime symlink");
        let profile = orb_profile(&path, &runtime, orb_uid(&runtime, &path));
        assert!(prepare_runtime(&home, &profile).is_err());
        drop(listener);
    }

    #[test]
    fn orb_runtime_metadata_size_is_bounded() {
        let temp = tempfile::TempDir::new().expect("temp");
        let home = temp.path().canonicalize().expect("canonical home");
        fs::create_dir(home.join("docker")).expect("docker config");
        let runtime = home.join("runtime");
        fs::create_dir(&runtime).expect("runtime");
        fs::create_dir(runtime.join("status")).expect("status");
        fs::write(runtime.join("vmgr.version"), vec![0_u8; 64 * 1024 + 1]).expect("version");
        let path = runtime.join("docker.sock");
        let listener = socket(&path);
        let profile = orb_profile(&path, &runtime, orb_uid(&runtime, &path));
        assert!(prepare_runtime(&home, &profile).is_err());
        drop(listener);
    }

    mod inventory_tests {
        include!("check_container_runtime_inventory_tests.rs");
    }
}
