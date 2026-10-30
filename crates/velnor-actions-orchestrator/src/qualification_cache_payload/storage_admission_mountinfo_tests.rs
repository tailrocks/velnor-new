use super::validate_read_only_mountinfo;

fn line(id: u64, target: &str, options: &str) -> String {
    format!("{id} 25 0:55 / {target} {options} rw,relatime - tmpfs tmpfs rw\n")
}

#[test]
fn mountinfo_requires_exact_read_only_mount_identity() {
    assert_eq!(
        validate_read_only_mountinfo(
            &line(42, "/opt/velnor/job-facts", "ro,nosuid,nodev"),
            "/opt/velnor/job-facts",
            42,
            0,
            55,
        ),
        Ok(())
    );
    assert_eq!(
        validate_read_only_mountinfo(
            &line(41, "/opt/velnor/job-facts", "ro"),
            "/opt/velnor/job-facts",
            42,
            0,
            55,
        ),
        Err("cache_storage_mount_not_read_only_bind")
    );
    assert_eq!(
        validate_read_only_mountinfo(
            &line(42, "/opt/velnor/job-facts", "rw,nosuid,nodev"),
            "/opt/velnor/job-facts",
            42,
            0,
            55,
        ),
        Err("cache_storage_mount_not_read_only_bind")
    );
}

#[test]
fn mountinfo_rejects_moved_covered_duplicate_and_malformed_targets() {
    let target = "/.velnor-runtime-identity";
    assert_eq!(
        validate_read_only_mountinfo(
            &line(42, "/opt/velnor-runtime-identity", "ro"),
            target,
            42,
            0,
            55,
        ),
        Err("cache_storage_mount_missing")
    );
    let duplicated = format!("{}{}", line(42, target, "ro"), line(42, target, "ro"));
    assert_eq!(
        validate_read_only_mountinfo(&duplicated, target, 42, 0, 55),
        Err("cache_storage_mount_ambiguous")
    );
    assert_eq!(
        validate_read_only_mountinfo(&line(42, target, "ro"), target, 42, 0, 55,),
        Ok(())
    );
    assert_eq!(
        validate_read_only_mountinfo(&line(42, target, "ro"), target, 42, 0, 56,),
        Err("cache_storage_mount_not_read_only_bind")
    );
}
