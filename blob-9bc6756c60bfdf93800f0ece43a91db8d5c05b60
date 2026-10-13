use super::*;

#[test]
fn orb_runtime_rejects_entry_sixty_five_before_admitting_its_path() {
    let temp = tempfile::TempDir::new().expect("temp");
    let home = temp.path().canonicalize().expect("canonical home");
    fs::create_dir(home.join("docker")).expect("docker config");
    let runtime = home.join("runtime");
    fs::create_dir(&runtime).expect("runtime");
    fs::create_dir(runtime.join("status")).expect("status");
    fs::write(runtime.join("vmgr.version"), b"1").expect("version");
    let path = runtime.join("docker.sock");
    let listener = socket(&path);
    let fixture_uid = home_uid(&runtime);
    let uid = orb_uid(&runtime, &path);
    for index in 0..63 {
        let child = runtime.join(format!("entry-{index:02}"));
        fs::write(&child, b"x").expect("runtime entry");
        if uid != fixture_uid {
            set_fixture_owner(&child, uid);
        }
    }
    let profile = orb_profile(&path, &runtime, uid);
    let error = prepare_runtime(&home, &profile).expect_err("65th entry refused");
    assert!(error.to_string().contains("orbstack_runtime_entry_limit"));
    drop(listener);
}
