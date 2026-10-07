//! Docker transport checks for worker identity and cleanup proof.

use std::time::Duration;

pub(super) const TIMEOUT: Duration = Duration::from_secs(2);
pub(super) const WORKER: &str = "wtransport";

pub(super) fn volume_names() -> [(&'static str, &'static str); 3] {
    [
        ("wtransport", "socket"),
        ("wtransport-work", "work"),
        ("wtransport-docker", "dind-data"),
    ]
}

pub(super) fn official_volume_names() -> [(&'static str, &'static str); 4] {
    [
        ("wtransport", "socket"),
        ("wtransport-work", "work"),
        ("wtransport-externals", "externals"),
        ("wtransport-docker", "dind-data"),
    ]
}

pub(super) fn volume_json(name: &str, worker: &str, role: &str) -> String {
    serde_json::json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
        "Labels": {"velnor.worker": worker, "velnor.role": role},
        "Options": {},
        "Scope": "local"
    })
    .to_string()
}

pub(super) fn container_json(id: &str, worker: Option<&str>, role: Option<&str>) -> String {
    let labels = match (worker, role) {
        (Some(worker), Some(role)) => serde_json::json!({
            "velnor.worker": worker,
            "velnor.volume": worker,
            "velnor.role": role
        }),
        _ => serde_json::json!({}),
    };
    serde_json::json!({
        "Id": id,
        "Config": {"Labels": labels},
        "HostConfig": {"CgroupnsMode": "private"}
    })
    .to_string()
}

#[cfg(all(test, unix))]
mod volumes_tests;
