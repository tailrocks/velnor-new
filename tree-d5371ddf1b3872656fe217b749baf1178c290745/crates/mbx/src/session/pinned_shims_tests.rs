#![cfg(unix)]
use super::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn private_directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}

fn executable(directory: &Path) -> PathBuf {
    let path = directory.join("owner");
    std::fs::write(&path, b"original executable").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn pin(directory: &Path, source: &Path) -> SessionDispatchPin {
    SessionDispatchPin {
        directory: directory.canonicalize().unwrap(),
        owner_sha256: digest(&mut File::open(source).unwrap()).unwrap(),
    }
}

#[test]
fn snapshots_survive_source_mutation_rename_and_upgrade() {
    let directory = private_directory();
    let source = executable(directory.path());
    let shim = directory.path().join("shim");
    install(&source, &shim).unwrap();
    let authority = pin(directory.path(), &source);
    assert_ne!(
        std::fs::metadata(&source).unwrap().ino(),
        std::fs::metadata(&shim).unwrap().ino()
    );
    std::fs::write(&source, b"modified in place").unwrap();
    authority.verify_route(&shim).unwrap();
    std::fs::rename(&source, directory.path().join("old-owner")).unwrap();
    std::fs::write(&source, b"replacement executable").unwrap();
    authority.verify_route(&shim).unwrap();
    assert_eq!(std::fs::read(shim).unwrap(), b"original executable");
}

#[test]
fn foreign_cached_file_and_alias_are_refused_without_replacement() {
    let directory = private_directory();
    let source = executable(directory.path());
    let shim = directory.path().join("shim");
    std::fs::write(&shim, b"foreign cached shim").unwrap();
    assert!(install(&source, &shim).is_err());
    assert_eq!(std::fs::read(&shim).unwrap(), b"foreign cached shim");
    std::fs::remove_file(&shim).unwrap();
    std::os::unix::fs::symlink(&source, &shim).unwrap();
    assert!(install(&source, &shim).is_err());
    assert!(
        std::fs::symlink_metadata(&shim)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&source).unwrap(), b"original executable");
}

#[test]
fn nonexecutable_and_alias_sources_are_rejected() {
    let directory = private_directory();
    let source = executable(directory.path());
    let shim = directory.path().join("shim");
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(install(&source, &shim).is_err());
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();
    let alias = directory.path().join("alias");
    std::os::unix::fs::symlink(&source, &alias).unwrap();
    assert!(install(&alias, &shim).is_err());
}

#[test]
fn retained_authority_rejects_mutation_replacement_nonexec_and_alias() {
    let directory = private_directory();
    let source = executable(directory.path());
    let shim = directory.path().join("shim");
    let authority = pin(directory.path(), &source);
    install(&source, &shim).unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(authority.verify_route(&shim).is_err());
    std::fs::write(&shim, b"modified snapshot").unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o555)).unwrap();
    assert!(authority.verify_route(&shim).is_err());
    std::fs::rename(&shim, directory.path().join("old-shim")).unwrap();
    std::fs::write(&shim, b"replacement snapshot").unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o555)).unwrap();
    assert!(authority.verify_route(&shim).is_err());
    assert!(install(&source, &shim).is_err());
    std::fs::remove_file(&shim).unwrap();
    install(&source, &shim).unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o444)).unwrap();
    assert!(authority.verify_route(&shim).is_err());
    std::fs::remove_file(&shim).unwrap();
    std::os::unix::fs::symlink(&source, &shim).unwrap();
    assert!(authority.verify_route(&shim).is_err());
}

#[test]
fn actual_installed_snapshots_bind_native_witness_and_revalidate() {
    use crate::dispatch_identity::{DispatchRoutes, NativeDispatchWitness, RouteConfiguration};
    use crate::session::completed_report::{CommandRole, SessionIdentity};
    let directory = private_directory();
    let installed = install_session(directory.path()).unwrap();
    let identity = SessionIdentity::new(CommandRole::CargoBuild, None, None, None).unwrap();
    let routes = DispatchRoutes {
        rustc: RouteConfiguration::managed(vec![installed.rustc.clone()]),
        cc: RouteConfiguration::disabled(),
        build_script: RouteConfiguration::disabled(),
        rustdoc: RouteConfiguration::managed(vec![installed.rustdoc.clone()]),
    };
    let witness = NativeDispatchWitness::verify(&identity, routes)
        .unwrap()
        .bind_snapshot_pin(&installed.dispatch_pin)
        .unwrap();
    assert!(witness.snapshot_pinning_verified());
    assert_eq!(
        witness.excluded_routes(),
        vec![
            mbx_cache_core::AdapterKind::Cc,
            mbx_cache_core::AdapterKind::BuildScript
        ]
    );
    let value = serde_json::to_value(&witness).unwrap();
    assert_eq!(value["snapshot_pinning"], "verified_session_snapshots");
    std::fs::set_permissions(&installed.rustc, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&installed.rustc, b"mutated installed snapshot").unwrap();
    assert!(witness.validate_current().is_err());
    assert!(!witness.snapshot_pinning_verified());
    assert!(serde_json::to_value(&witness).is_err());
}

#[test]
fn retained_authority_rejects_hardlink_and_parent_aliases() {
    let directory = private_directory();
    let source = executable(directory.path());
    let shim = directory.path().join("shim");
    install(&source, &shim).unwrap();
    let authority = pin(directory.path(), &source);
    let alias = directory.path().join("hardlink");
    std::fs::hard_link(&shim, &alias).unwrap();
    assert!(authority.verify_route(&shim).is_err());
    std::fs::remove_file(alias).unwrap();
    authority.verify_route(&shim).unwrap();
    let parent = private_directory();
    let alias_directory = parent.path().join("alias");
    std::os::unix::fs::symlink(directory.path(), &alias_directory).unwrap();
    assert!(
        authority
            .verify_route(&alias_directory.join("shim"))
            .is_err()
    );
}

#[test]
fn actual_installer_normalizes_parent_alias_before_minting_authority() {
    use crate::dispatch_identity::{DispatchRoutes, NativeDispatchWitness, RouteConfiguration};
    use crate::session::completed_report::{CommandRole, SessionIdentity};
    let directory = private_directory();
    let aliases = private_directory();
    let alias = aliases.path().join("session");
    std::os::unix::fs::symlink(directory.path(), &alias).unwrap();
    let installed = install_session(&alias.join("shims")).unwrap();
    assert_eq!(installed.rustc.canonicalize().unwrap(), installed.rustc);
    assert_eq!(installed.rustdoc.canonicalize().unwrap(), installed.rustdoc);
    assert_eq!(installed.native.canonicalize().unwrap(), installed.native);
    let identity = SessionIdentity::new(CommandRole::CargoBuild, None, None, None).unwrap();
    let routes = DispatchRoutes {
        rustc: RouteConfiguration::managed(vec![installed.rustc.clone()]),
        cc: RouteConfiguration::disabled(),
        build_script: RouteConfiguration::disabled(),
        rustdoc: RouteConfiguration::managed(vec![installed.rustdoc.clone()]),
    };
    let witness = NativeDispatchWitness::verify(&identity, routes)
        .unwrap()
        .bind_snapshot_pin(&installed.dispatch_pin)
        .unwrap();
    assert!(witness.snapshot_pinning_verified());
    assert!(
        installed
            .dispatch_pin
            .verify_route(
                &alias.join("shims").join(
                    installed
                        .rustc
                        .strip_prefix(directory.path().join("shims"))
                        .unwrap()
                )
            )
            .is_err()
    );
}
