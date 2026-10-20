use super::*;
use std::fs;

const RUN_KEY: &str = "r700-a1";

#[test]
fn runner_temp_symlink_is_rejected() {
    let temp = tempfile::tempdir().expect("The test could not create a temporary directory.");
    let real = temp.path().join("real");
    fs::create_dir(&real).expect("The test could not create the real directory.");
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias)
        .expect("The test could not create the parent symbolic link.");

    let result = RunDirectory::open(&alias, RUN_KEY);
    assert!(
        result.is_err(),
        "The opener must reject a symbolic link at the temporary root."
    );
}

#[test]
fn parent_symlink_swap_cannot_redirect_the_open_run_directory() {
    let temp = tempfile::tempdir().expect("The test could not create a temporary directory.");
    let root = temp
        .path()
        .canonicalize()
        .expect("The test could not get the canonical temporary root.");
    let real_parent = root.join("real");
    let other_parent = root.join("other");
    let runner_temp = real_parent.join("runner-temp");
    let run_dir = runner_temp.join("velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("The test could not create the run directory.");
    fs::create_dir_all(other_parent.join("runner-temp/velnor").join(RUN_KEY))
        .expect("The test could not create the other runner temporary directory.");
    fs::write(run_dir.join("runtime-identity.json"), b"original")
        .expect("The test could not write the original file.");
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&real_parent, &alias)
        .expect("The test could not create the parent symbolic link.");
    let alias_runner_temp = alias.join("runner-temp");
    let opened = RunDirectory::open(&alias_runner_temp, RUN_KEY)
        .expect("The test could not open the run directory.");

    fs::remove_file(&alias).expect("The test could not remove the parent symbolic link.");
    std::os::unix::fs::symlink(&other_parent, &alias)
        .expect("The test could not replace the parent symbolic link.");
    fs::write(
        other_parent
            .join("runner-temp/velnor")
            .join(RUN_KEY)
            .join("runtime-identity.json"),
        b"replacement",
    )
    .expect("The test could not write the replacement file.");

    let bytes = opened
        .read_owned_file(
            "runtime-identity.json",
            64,
            None,
            "qualification_cache_runtime_identity",
        )
        .expect("The test could not read the file through the open run directory.");
    assert_eq!(bytes, b"original");
}

#[test]
fn opened_run_directory_stays_pinned_after_path_replacement() {
    let temp = tempfile::tempdir().expect("The test could not create a temporary directory.");
    let runner_temp = temp.path().join("runner-temp");
    let run_dir = runner_temp.join("velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("The test could not create the run directory.");
    fs::write(run_dir.join("runtime-identity.json"), b"original")
        .expect("The test could not write the original file.");
    let opened = RunDirectory::open(&runner_temp, RUN_KEY)
        .expect("The test could not open the run directory.");

    let moved = runner_temp.join("velnor").join("moved");
    fs::rename(&run_dir, &moved).expect("The test could not move the original run directory.");
    fs::create_dir(&run_dir).expect("The test could not create the replacement run directory.");
    fs::write(run_dir.join("runtime-identity.json"), b"replacement")
        .expect("The test could not write the replacement file.");

    let bytes = opened
        .read_owned_file(
            "runtime-identity.json",
            64,
            None,
            "qualification_cache_runtime_identity",
        )
        .expect("The test could not read the file through the open run directory.");
    assert_eq!(bytes, b"original");
}

#[cfg(unix)]
#[test]
fn owned_file_mode_check_accepts_expected_mode_and_rejects_a_different_mode() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("The test cannot create a temporary directory.");
    let run_dir = temp.path().join("velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("The test cannot create the run directory.");
    let file_path = run_dir.join("runtime-identity.json");
    fs::write(&file_path, b"{}").expect("The test cannot write the identity file.");
    fs::set_permissions(&file_path, fs::Permissions::from_mode(0o600))
        .expect("The test cannot set file mode 0600.");

    let opened =
        RunDirectory::open(temp.path(), RUN_KEY).expect("The test cannot open the run directory.");
    assert_eq!(
        opened
            .read_owned_file(
                "runtime-identity.json",
                16,
                Some(0o600),
                "qualification_cache_runtime_identity",
            )
            .expect("The reader must accept file mode 0600."),
        b"{}"
    );

    fs::set_permissions(&file_path, fs::Permissions::from_mode(0o644))
        .expect("The test cannot set file mode 0644.");
    assert!(
        opened
            .read_owned_file(
                "runtime-identity.json",
                16,
                Some(0o600),
                "qualification_cache_runtime_identity",
            )
            .is_err(),
        "The reader must reject file mode 0644 when it accepts only file mode 0600."
    );
}
