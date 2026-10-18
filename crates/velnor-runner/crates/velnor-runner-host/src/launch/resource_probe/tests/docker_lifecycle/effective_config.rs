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
    Ok(())
}

#[tokio::test]
async fn engine29_container_inspect_defaults_match_the_fixed_profile() -> Result<(), String> {
    let (_scratch, _journal, projection) = prepared().await?;
    // Captured from Docker Engine 29.4.0; only IDs and the fixture bind are replaced.
    let mut actual = serde_json::from_str::<Value>(include_str!("engine29_container_inspect.json"))
        .map_err(|error| error.to_string())?;
    actual["Id"] = json!(CONTAINER_ID);
    actual["Name"] = json!(format!("/{}", projection.name));
    actual["Image"] = json!(projection.image.runtime_id());
    actual["Config"]["Image"] = json!(projection.image.runtime_id());
    actual["Config"]["Hostname"] = json!(&CONTAINER_ID[..12]);
    actual["Config"]["Labels"] = json!(projection.config.labels.as_ref());
    actual["HostConfig"]["Mounts"][0]["Source"] = json!(projection.root.path());
    actual["Mounts"][0]["Source"] = json!(projection.root.path());
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
