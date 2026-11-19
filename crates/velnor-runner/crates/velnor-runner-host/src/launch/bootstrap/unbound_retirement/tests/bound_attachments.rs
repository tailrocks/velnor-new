// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn attachment_statements(
    reservation: i64,
    runner: &str,
    dind: &str,
    digest: &str,
) -> Vec<String> {
    let mut statements = Vec::new();
    for (mount_id, (role, observer, container, target, source, read_only)) in [
        ("shared-run", "runner", Some(runner), "/run", None, 0),
        (
            "runner-home",
            "runner",
            Some(runner),
            "/home/runner",
            None,
            0,
        ),
        (
            "runner-work",
            "runner",
            Some(runner),
            "/home/runner/_work",
            None,
            0,
        ),
        ("runner-tmp", "runner", Some(runner), "/tmp", None, 0),
        ("runner-shm", "runner", Some(runner), "/dev/shm", None, 0),
        ("dind-data", "dind", Some(dind), "/var/lib/docker", None, 0),
        ("dind-tmp", "dind", Some(dind), "/tmp", None, 0),
        ("dind-shm", "dind", Some(dind), "/dev/shm", None, 0),
        ("shared-run", "dind", Some(dind), "/run", None, 0),
        ("runner-home", "dind", Some(dind), "/home/runner", None, 0),
        (
            "runner-work",
            "dind",
            Some(dind),
            "/home/runner/_work",
            None,
            0,
        ),
        ("runner-root", "runner", Some(runner), "/", None, 0),
        ("dind-root", "dind", Some(dind), "/", None, 0),
        (
            "engine-metadata",
            "probe",
            None,
            "/velnor/docker-root",
            Some("/var/lib/docker"),
            1,
        ),
        (
            "engine-metadata",
            "runner",
            Some(runner),
            "/.velnor-runtime-identity",
            None,
            1,
        ),
        (
            "engine-metadata",
            "dind",
            Some(dind),
            "/.velnor-runtime-identity",
            None,
            1,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let container_sql = container.map_or("NULL".to_owned(), |text| format!("'{text}'"));
        let source_sql = source.map_or("NULL".to_owned(), |text| format!("'{text}'"));
        let id = mount_id + 1;
        statements.push(format!("INSERT INTO capacity_reservation_bound_storage_attachments (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, filesystem_id, filesystem_instance_digest) VALUES ({reservation}, '{role}', '{observer}', {container_sql}, '{target}', {source_sql}, {read_only}, {id}, 0, {id}, 0, {id}, 1, '0000000000000001', '{digest}')"));
    }
    statements
}
