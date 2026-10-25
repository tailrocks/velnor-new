use super::{RUN_KEY, fixture, validate_runner_directories};
use std::fs;

#[test]
fn staged_tree_directory_links_are_rejected() {
    let (temp, _, _, _) = fixture();
    fs::create_dir(temp.path().join("velnor")).expect("parent");
    #[cfg(unix)]
    std::os::unix::fs::symlink(temp.path(), temp.path().join("velnor/r720-a1"))
        .expect("run symlink");
    #[cfg(unix)]
    let error =
        validate_runner_directories(temp.path(), RUN_KEY, None).expect_err("run symlink rejected");
    #[cfg(unix)]
    assert!(
        error
            .to_string()
            .contains("qualification_source_directory_untrusted")
    );
}

#[cfg(unix)]
#[test]
fn matrix_and_task_directories_cannot_redirect_report_reads() {
    use std::os::unix::fs::symlink;

    let (matrix_temp, _, entry, _) = fixture();
    let run_dir = matrix_temp.path().join("velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir).expect("run directory");
    symlink(matrix_temp.path(), run_dir.join(&entry.matrix_key)).expect("matrix symlink");
    let error = validate_runner_directories(matrix_temp.path(), RUN_KEY, Some(&entry.matrix_key))
        .expect_err("matrix symlink is rejected");
    assert!(
        error
            .to_string()
            .contains("qualification_source_directory_untrusted")
    );

    let (task_temp, _, entry, _) = fixture();
    let matrix_dir = task_temp
        .path()
        .join("velnor")
        .join(RUN_KEY)
        .join(&entry.matrix_key);
    fs::create_dir_all(&matrix_dir).expect("matrix directory");
    symlink(task_temp.path(), matrix_dir.join("tasks")).expect("task symlink");
    let error = validate_runner_directories(task_temp.path(), RUN_KEY, Some(&entry.matrix_key))
        .expect_err("task symlink is rejected");
    assert!(
        error
            .to_string()
            .contains("qualification_source_directory_untrusted")
    );
}
