use super::*;
use bollard::models::ContainerInspectResponse;
use serde_json::{Value, json};

#[tokio::test]
async fn inspect_config_includes_inherited_profile_and_rejects_drift() -> Result<(), String> {
    let (_scratch, _journal, projection) = prepared().await?;
    let config = serde_json::from_str::<Value>(&inspect(&projection, "created", 0)?)
        .map_err(|error| error.to_string())?;
    let response = serde_json::from_value::<ContainerInspectResponse>(config.clone())
        .map_err(|error| error.to_string())?;
    assert!(crate::launch::resource_probe::inspect::matches(
        &projection,
        &response,
        CONTAINER_ID
    ));

    for (field, value) in [
        ("Env", json!(["PATH=/usr/local/bin", "UNEXPECTED=1"])),
        ("NetworkDisabled", json!(false)),
        ("WorkingDir", json!("/tmp")),
        ("Entrypoint", json!(["/tmp/untrusted-probe"])),
        ("HostConfig.Memory", json!(64 * 1024 * 1024)),
        ("HostConfig.MemorySwap", json!(256 * 1024 * 1024)),
        ("HostConfig.PidsLimit", json!(0)),
        ("HostConfig.CpuQuota", json!(20_000)),
        ("HostConfig.PidMode", json!("private")),
        ("HostConfig.CgroupnsMode", json!("private")),
    ] {
        let mut changed = config.clone();
        let mut path = field.split('.');
        let root = path
            .next()
            .ok_or_else(|| "missing config root".to_owned())?;
        let leaf = path.next();
        if let Some(leaf) = leaf {
            changed[root][leaf] = value;
        } else {
            changed["Config"][field] = value;
        }
        let response = serde_json::from_value::<ContainerInspectResponse>(changed)
            .map_err(|error| error.to_string())?;
        assert!(!crate::launch::resource_probe::inspect::matches(
            &projection,
            &response,
            CONTAINER_ID
        ));
    }
    let mut missing_cgroup_bind = config.clone();
    missing_cgroup_bind["HostConfig"]["Mounts"] =
        json!([config["HostConfig"]["Mounts"][0].clone()]);
    missing_cgroup_bind["Mounts"] = json!([config["Mounts"][0].clone()]);
    let missing_cgroup_bind =
        serde_json::from_value::<ContainerInspectResponse>(missing_cgroup_bind)
            .map_err(|error| error.to_string())?;
    assert!(!crate::launch::resource_probe::inspect::matches(
        &projection,
        &missing_cgroup_bind,
        CONTAINER_ID
    ));
    Ok(())
}

#[tokio::test]
async fn engine29_container_inspect_defaults_match_the_fixed_profile() -> Result<(), String> {
    let (_scratch, _journal, projection) = prepared().await?;
    // External fixture: Docker Engine 29.4.0. This test sets identities, PidMode, CgroupnsMode, and bind mounts.
    let mut actual = serde_json::from_str::<Value>(include_str!("engine29_container_inspect.json"))
        .map_err(|error| error.to_string())?;
    actual["Id"] = json!(CONTAINER_ID);
    actual["Name"] = json!(format!("/{}", projection.name));
    actual["Image"] = json!(projection.image.runtime_id());
    actual["Config"]["Image"] = json!(projection.image.runtime_id());
    actual["Config"]["Hostname"] = json!(&CONTAINER_ID[..12]);
    actual["Config"]["Labels"] = json!(projection.config.labels.as_ref());
    actual["HostConfig"]["Mounts"][0]["Source"] = json!(projection.root.path());
    actual["HostConfig"]["CgroupnsMode"] = json!("host");
    actual["HostConfig"]["PidMode"] = json!("host");
    actual["HostConfig"]["Mounts"]
        .as_array_mut()
        .ok_or_else(|| "HostConfig.Mounts is not a JSON array.".to_owned())?
        .push(json!({
            "Target": crate::launch::resource_probe::projection::CGROUP_TARGET,
            "Source": crate::launch::resource_probe::projection::CGROUP_TARGET,
            "Type": "bind",
            "ReadOnly": true
        }));
    actual["Mounts"][0]["Source"] = json!(projection.root.path());
    actual["Mounts"]
        .as_array_mut()
        .ok_or_else(|| "Mounts is not a JSON array.".to_owned())?
        .push(json!({
            "Type": "bind",
            "Source": crate::launch::resource_probe::projection::CGROUP_TARGET,
            "Destination": crate::launch::resource_probe::projection::CGROUP_TARGET,
            "RW": false
        }));
    let response = serde_json::from_value::<ContainerInspectResponse>(actual)
        .map_err(|error| error.to_string())?;

    assert!(crate::launch::resource_probe::inspect::matches(
        &projection,
        &response,
        CONTAINER_ID
    ));
    assert!(crate::launch::resource_probe::inspect::exited_successfully(
        &response
    ));
    Ok(())
}
