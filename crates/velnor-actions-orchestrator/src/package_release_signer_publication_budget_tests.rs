use super::*;
use std::time::Duration;
use velnor_actions_mise::CheckDeadline;

#[test]
fn publication_signer_custody_keeps_the_supplied_deadline_without_resetting_it() {
    let temp_root = std::env::current_dir()
        .expect("package working directory")
        .join(".publication-test-tmp");
    std::fs::create_dir_all(&temp_root).expect("test temp root");
    let directory = tempfile::tempdir_in(&temp_root).expect("signed input directory");
    let path = directory
        .path()
        .canonicalize()
        .expect("canonical input directory");
    std::fs::write(path.join("asset.tar.gz"), b"signed bytes").expect("signed input file");
    let deadline = CheckDeadline::after(Duration::from_millis(500)).expect("custody deadline");
    let ((), custody) =
        hold_signed_directory_for_publication(&path, deadline, || Ok(())).expect("capture bytes");
    custody.verify().expect("custody initially valid");
    std::thread::sleep(Duration::from_millis(550));
    assert!(
        custody.verify().is_err(),
        "custody uses the original deadline"
    );
    drop(directory);
    std::fs::remove_dir(&temp_root).expect("remove test temp root");
}
