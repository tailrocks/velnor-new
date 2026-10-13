use super::*;

/// Scratch dir plus a display-safe join.
fn scratch(name: &str) -> tempfile::TempDir {
    tempfile::TempDir::with_prefix(format!("exclusive-{name}-")).expect("tempdir")
}

#[test]
fn exclusive_write_round_trips_and_refuses_rewrite() {
    let temp = scratch("roundtrip");
    let file = temp.path().join("report.json");
    write_exclusive(&file, b"{}", "report").expect("first write");
    assert_eq!(fs::read(&file).expect("readback"), b"{}");
    let err = write_exclusive(&file, b"{}", "report").expect_err("rewrite refused");
    assert!(err.to_string().contains("report_exists"), "{err}");
}

#[cfg(unix)]
#[test]
fn exclusive_write_refuses_planted_symlinks() {
    let temp = scratch("symlink");
    let target = temp.path().join("target.json");
    let link = temp.path().join("report.json");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    let err = write_exclusive(&link, b"{}", "report").expect_err("link refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
    assert!(!target.exists(), "bytes never followed the plant");
}

#[cfg(unix)]
#[test]
fn dir_creation_refuses_symlinked_components() {
    let temp = scratch("dirlink");
    let real = temp.path().join("real");
    fs::create_dir(&real).expect("real dir");
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let err =
        create_dir_no_symlink(temp.path(), &link.join("sub")).expect_err("linked parent refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
    assert!(
        !real.join("sub").exists(),
        "refusal must not create through the plant"
    );
    let clean = temp.path().join("a").join("b");
    create_dir_no_symlink(temp.path(), &clean).expect("clean parents pass");
    assert!(clean.is_dir());
    create_dir_no_symlink(temp.path(), &clean).expect("re-create is idempotent");
}

#[test]
fn dir_creation_refuses_anchor_escapes() {
    let temp = scratch("escape");
    let elsewhere = scratch("elsewhere");
    let err = create_dir_no_symlink(temp.path(), &elsewhere.path().join("sub"))
        .expect_err("outside anchor refused");
    assert!(err.to_string().contains("anchor_escape"), "{err}");
    let err = create_dir_no_symlink(
        temp.path(),
        &temp.path().join("sub").join("..").join("sneaky"),
    )
    .expect_err("dot-dot refused");
    assert!(err.to_string().contains("anchor_escape"), "{err}");
    assert!(
        !temp.path().join("sneaky").exists(),
        "escape must not create"
    );
}

#[cfg(unix)]
#[test]
fn exclusive_write_refuses_symlinked_parent() {
    let temp = scratch("parentlink");
    let real = temp.path().join("real");
    fs::create_dir(&real).expect("real dir");
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let err = write_exclusive(&link.join("report.json"), b"{}", "report")
        .expect_err("linked parent refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
    assert!(
        !real.join("report.json").exists(),
        "bytes never followed the plant"
    );
    let missing = temp.path().join("nope").join("report.json");
    let err = write_exclusive(&missing, b"{}", "report").expect_err("missing parent refused");
    assert!(err.to_string().contains("report_unwritable"), "{err}");
}
