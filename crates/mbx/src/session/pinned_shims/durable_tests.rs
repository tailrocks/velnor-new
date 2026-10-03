use super::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::{Arc, Barrier};

fn directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}

fn source(directory: &Path) -> PathBuf {
    let path = directory.join("source");
    std::fs::write(&path, b"first owner executable").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn durable_paths_survive_installation_release_and_are_reused() {
    let cache = directory();
    let source = source(cache.path());
    let configured = cache.path().join("shims");
    let first = install_session_from(&configured, &source).unwrap();
    let path = first.rustc.clone();
    let inode = std::fs::metadata(&path).unwrap().ino();
    drop(first);
    assert!(path.is_file());
    let second = install_session_from(&configured, &source).unwrap();
    assert_eq!(second.rustc, path);
    assert_eq!(std::fs::metadata(&second.rustc).unwrap().ino(), inode);
    second.dispatch_pin.verify_route(&second.rustc).unwrap();
}

#[test]
fn upgraded_source_gets_distinct_namespace_and_old_lazy_roles_keep_first_bytes() {
    let cache = directory();
    let source = source(cache.path());
    let configured = cache.path().join("shims");
    let first = install_session_from(&configured, &source).unwrap();
    std::fs::write(&source, b"second owner executable").unwrap();
    let second = install_session_from(&configured, &source).unwrap();
    assert_ne!(first.rustc, second.rustc);
    let old_cc = first.native.join("cc");
    install(&source, &old_cc).unwrap();
    assert_eq!(std::fs::read(&old_cc).unwrap(), b"first owner executable");
    first.dispatch_pin.verify_route(&old_cc).unwrap();
    let new_cc = second.native.join("cc");
    install(&source, &new_cc).unwrap();
    assert_eq!(std::fs::read(&new_cc).unwrap(), b"second owner executable");
    second.dispatch_pin.verify_route(&new_cc).unwrap();
}

#[test]
fn concurrent_first_installations_share_exact_path_and_inode() {
    let cache = directory();
    let source = source(cache.path());
    let configured = cache.path().join("shims");
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (source, configured, barrier) =
                (source.clone(), configured.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let installed = install_session_from(&configured, &source).unwrap();
                installed
                    .dispatch_pin
                    .verify_route(&installed.rustc)
                    .unwrap();
                (
                    installed.rustc.clone(),
                    std::fs::metadata(installed.rustc).unwrap().ino(),
                )
            })
        })
        .collect();
    let installed: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(installed.iter().all(|value| value == &installed[0]));
    assert_eq!(
        std::fs::read(&installed[0].0).unwrap(),
        b"first owner executable"
    );
}

#[test]
fn concurrent_lazy_publication_retains_one_independent_inode() {
    let cache = directory();
    let source = source(cache.path());
    let installed = install_session_from(&cache.path().join("shims"), &source).unwrap();
    let destination = installed.native.join("cc");
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (source, destination, barrier) =
                (source.clone(), destination.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                install(&source, &destination).unwrap();
                std::fs::metadata(destination).unwrap().ino()
            })
        })
        .collect();
    let inodes: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(inodes.iter().all(|inode| inode == &inodes[0]));
    assert_ne!(inodes[0], std::fs::metadata(&source).unwrap().ino());
    assert_eq!(std::fs::metadata(&destination).unwrap().nlink(), 1);
    installed.dispatch_pin.verify_route(&destination).unwrap();
}

#[test]
fn corrupted_captured_owner_is_refused_and_never_repaired() {
    let cache = directory();
    let source = source(cache.path());
    let configured = cache.path().join("shims");
    let installed = install_session_from(&configured, &source).unwrap();
    let owner = installed.dispatch_pin.directory.join(durable::OWNER_FILE);
    std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(&owner, b"corrupt captured owner").unwrap();
    std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o500)).unwrap();
    let inode = std::fs::metadata(&owner).unwrap().ino();
    assert!(install_session_from(&configured, &source).is_err());
    assert!(install(&source, &installed.native.join("cc")).is_err());
    assert_eq!(std::fs::metadata(&owner).unwrap().ino(), inode);
    assert_eq!(std::fs::read(&owner).unwrap(), b"corrupt captured owner");
}

#[test]
fn equal_size_corrupted_cached_role_is_refused_without_replacement() {
    let cache = directory();
    let source = source(cache.path());
    let configured = cache.path().join("shims");
    let installed = install_session_from(&configured, &source).unwrap();
    let inode = std::fs::metadata(&installed.rustc).unwrap().ino();
    let mut bytes = std::fs::read(&installed.rustc).unwrap();
    bytes[0] ^= 1;
    std::fs::set_permissions(&installed.rustc, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(&installed.rustc, &bytes).unwrap();
    std::fs::set_permissions(&installed.rustc, std::fs::Permissions::from_mode(0o500)).unwrap();
    assert!(install_session_from(&configured, &source).is_err());
    assert_eq!(std::fs::metadata(&installed.rustc).unwrap().ino(), inode);
    assert_eq!(std::fs::read(installed.rustc).unwrap(), bytes);
}

#[test]
fn writable_or_setid_cached_role_is_refused() {
    let cache = directory();
    let source = source(cache.path());
    let configured = cache.path().join("shims");
    let installed = install_session_from(&configured, &source).unwrap();
    for mode in [0o700, 0o4500, 0o2500, 0o1500] {
        std::fs::set_permissions(&installed.rustc, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(install_session_from(&configured, &source).is_err());
        assert_eq!(
            std::fs::metadata(&installed.rustc).unwrap().mode() & 0o7777,
            mode
        );
    }
}

#[test]
fn wrong_captured_digest_never_publishes_snapshot() {
    let cache = directory();
    let source = source(cache.path());
    let destination = cache.path().join("snapshot");
    assert!(install_expected(&source, &destination, &"0".repeat(64)).is_err());
    assert!(!destination.exists());
}

#[test]
fn mutation_during_actual_copy_refuses_publication_and_removes_owned_stage() {
    let cache = directory();
    let source = source(cache.path());
    std::fs::write(&source, vec![1u8; 256 * 1024]).unwrap();
    let expected = digest(&mut File::open(&source).unwrap()).unwrap();
    let destination = cache.path().join("snapshot");
    let mut mutated = false;
    let result = install_expected_with(&source, &destination, &expected, || {
        mutated = true;
        std::fs::write(&source, vec![2u8; 256 * 1024])?;
        Ok(())
    });
    assert!(mutated);
    assert!(result.is_err());
    assert!(!destination.exists());
    let files: Vec<_> = std::fs::read_dir(cache.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files, vec![std::ffi::OsString::from("source")]);
}
