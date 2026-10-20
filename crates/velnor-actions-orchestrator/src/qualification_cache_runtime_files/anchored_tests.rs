use super::*;
use std::fs;

const RUN_KEY: &str = "r700-a1";

#[test]
fn runner_temp_symlink_is_rejected() {
    let temp = tempfile::tempdir().expect("temp");
    let real = temp.path().join("real");
    fs::create_dir(&real).expect("real directory");
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).expect("parent symlink");

    let result = RunDirectory::open(&alias, RUN_KEY);
    assert!(result.is_err(), "parent symlink must fail closed");
}

#[test]
fn parent_symlink_swap_cannot_redirect_the_open_run_directory() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().canonicalize().expect("canonical temp root");
    let real_parent = root.join("real");
    let other_parent = root.join("other");
    let runner_temp = real_parent.join("runner-temp");
    let run_dir = runner_temp.join("velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("run directory");
    fs::create_dir_all(other_parent.join("runner-temp/velnor").join(RUN_KEY))
        .expect("other runner temp");
    fs::write(run_dir.join("runtime-identity.json"), b"original").expect("original file");
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&real_parent, &alias).expect("parent alias");
    let alias_runner_temp = alias.join("runner-temp");
    let opened = RunDirectory::open(&alias_runner_temp, RUN_KEY).expect("open run directory");

    fs::remove_file(&alias).expect("remove alias");
    std::os::unix::fs::symlink(&other_parent, &alias).expect("replace alias");
    fs::write(
        other_parent
            .join("runner-temp/velnor")
            .join(RUN_KEY)
            .join("runtime-identity.json"),
        b"replacement",
    )
    .expect("replacement file");

    let bytes = opened
        .read_owned_file(
            "runtime-identity.json",
            64,
            None,
            "qualification_cache_runtime_identity",
        )
        .expect("read from pinned directory");
    assert_eq!(bytes, b"original");
}

#[test]
fn opened_run_directory_stays_pinned_after_path_replacement() {
    let temp = tempfile::tempdir().expect("temp");
    let runner_temp = temp.path().join("runner-temp");
    let run_dir = runner_temp.join("velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("run directory");
    fs::write(run_dir.join("runtime-identity.json"), b"original").expect("original file");
    let opened = RunDirectory::open(&runner_temp, RUN_KEY).expect("open run directory");

    let moved = runner_temp.join("velnor").join("moved");
    fs::rename(&run_dir, &moved).expect("move original run directory");
    fs::create_dir(&run_dir).expect("replacement run directory");
    fs::write(run_dir.join("runtime-identity.json"), b"replacement").expect("replacement file");

    let bytes = opened
        .read_owned_file(
            "runtime-identity.json",
            64,
            None,
            "qualification_cache_runtime_identity",
        )
        .expect("read from pinned directory");
    assert_eq!(bytes, b"original");
}

#[cfg(unix)]
#[test]
fn owner_layer_evidence_is_created_private_and_never_overwritten() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("temporary directory");
    let run_dir = temp.path().join("runner-temp/velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("run directory");
    let opened =
        RunDirectory::open(&temp.path().join("runner-temp"), RUN_KEY).expect("open run directory");
    opened
        .write_owned_file(
            "runtime-identity-rust-demo-cargo_sources.json",
            b"{}",
            "identity",
        )
        .expect("write identity");
    let path = run_dir.join("runtime-identity-rust-demo-cargo_sources.json");
    assert_eq!(
        fs::metadata(&path)
            .expect("identity metadata")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    assert_eq!(
        opened
            .read_owned_file(
                "runtime-identity-rust-demo-cargo_sources.json",
                64,
                Some(0o600),
                "identity",
            )
            .expect("read identity"),
        b"{}"
    );
    assert!(
        opened
            .write_owned_file(
                "runtime-identity-rust-demo-cargo_sources.json",
                b"{}",
                "identity"
            )
            .is_err()
    );
}

#[test]
fn owner_layer_evidence_writer_rejects_nested_paths() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let run_dir = temp.path().join("runner-temp/velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("run directory");
    let opened =
        RunDirectory::open(&temp.path().join("runner-temp"), RUN_KEY).expect("open run directory");
    assert!(
        opened
            .write_owned_file("../escape.json", b"{}", "identity")
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn relative_reader_accepts_private_files_and_rejects_group_readable_files() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("temporary directory");
    let run_dir = temp.path().join("runner-temp/velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("run directory");
    let opened =
        RunDirectory::open(&temp.path().join("runner-temp"), RUN_KEY).expect("open run directory");
    opened
        .write_owned_relative_file("qualification-cache/runtime.json", b"{}", "identity")
        .expect("write relative identity");

    let path = run_dir.join("qualification-cache/runtime.json");
    let mut permissions = fs::metadata(&path)
        .expect("identity metadata")
        .permissions();
    assert_eq!(permissions.mode() & 0o7777, 0o600);
    assert_eq!(
        opened
            .read_owned_relative_file(
                "qualification-cache/runtime.json",
                64,
                Some(0o600),
                "identity",
            )
            .expect("read private identity"),
        b"{}"
    );

    permissions.set_mode(0o644);
    fs::set_permissions(&path, permissions).expect("set readable identity mode");
    assert!(
        opened
            .read_owned_relative_file(
                "qualification-cache/runtime.json",
                64,
                Some(0o600),
                "identity",
            )
            .is_err()
    );
}
