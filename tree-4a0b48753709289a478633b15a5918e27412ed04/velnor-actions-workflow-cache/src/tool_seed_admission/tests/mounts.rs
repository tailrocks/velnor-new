use super::{run, run_with_file_size, scratch, seed, trust_script};
use std::fs;

#[test]
fn only_one_exact_read_only_non_overlay_mount_is_admitted() {
    let test_dir = scratch("mount");
    let seed_root = test_dir.join("seed");
    seed(&seed_root);
    let script = trust_script(&test_dir);
    let exact = format!("{} ext4 0:77 ro,nosuid,nodev", seed_root.display());
    assert!(run(&script, &seed_root, &exact).status.success());

    let denied_mounts = [
        format!("{} ext4 0:77 rw,nosuid", seed_root.display()),
        format!("{} overlay 0:77 ro", seed_root.display()),
        format!(
            "{} ext4 0:77 ro\n{}/child ext4 0:78 ro",
            seed_root.display(),
            seed_root.display()
        ),
        format!("/ ext4 0:77 rw\n{} ext4 0:77 ro", seed_root.display()),
        String::new(),
    ];
    for mounts in denied_mounts {
        let output = run(&script, &seed_root, &mounts);
        assert!(!output.status.success(), "accepted mounts: {mounts}");
    }
    fs::remove_dir_all(test_dir).ok();
}

#[test]
fn provenance_marker_rejects_oversize_nul_and_extra_lines() {
    let test_dir = scratch("provenance");
    let seed_root = test_dir.join("seed");
    seed(&seed_root);
    let script = trust_script(&test_dir);
    let mounts = format!("{} ext4 0:77 ro", seed_root.display());
    assert!(run(&script, &seed_root, &mounts).status.success());

    fs::write(seed_root.join("PROVENANCE"), b"velnor-host-seed-v1\0").expect("NUL marker");
    assert!(!run(&script, &seed_root, &mounts).status.success());
    fs::write(seed_root.join("PROVENANCE"), "velnor-host-seed-v1\nextra\n")
        .expect("multi-line marker");
    assert!(!run(&script, &seed_root, &mounts).status.success());
    fs::write(seed_root.join("PROVENANCE"), "velnor-host-seed-v1\n").expect("marker");
    assert!(
        !run_with_file_size(&script, &seed_root, &mounts, "513")
            .status
            .success()
    );
    fs::remove_dir_all(test_dir).expect("cleanup");
}
