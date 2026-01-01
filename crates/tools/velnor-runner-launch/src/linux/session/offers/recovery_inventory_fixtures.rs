use std::collections::BTreeMap;

use velnor_runner_host::IntentRow;
use velnor_runner_host::worker::{OwnedDockerResource, OwnedDockerResourceKind};

pub(super) fn complete_inventory(row: &IntentRow) -> Result<Vec<OwnedDockerResource>, String> {
    let worker = row
        .worker_volume
        .as_deref()
        .ok_or_else(|| "fixture has no worker volume".to_owned())?;
    let runner = row
        .docker_id
        .as_deref()
        .ok_or_else(|| "fixture has no runner container".to_owned())?;
    let dind = row
        .dind_id
        .as_deref()
        .ok_or_else(|| "fixture has no DinD container".to_owned())?;
    let network = row
        .outer_network_id
        .as_deref()
        .ok_or_else(|| "fixture has no outer network".to_owned())?;
    let network_name = row
        .outer_network_name
        .as_deref()
        .ok_or_else(|| "fixture has no outer network name".to_owned())?;
    let mut resources = vec![
        resource(
            OwnedDockerResourceKind::Container,
            runner,
            worker,
            "runner",
            worker,
        ),
        resource(
            OwnedDockerResourceKind::Container,
            dind,
            worker,
            "dind",
            worker,
        ),
        OwnedDockerResource {
            kind: OwnedDockerResourceKind::Network,
            id_or_name: network.to_owned(),
            names: vec![network_name.to_owned()],
            worker: worker.to_owned(),
            role: "outer-network".to_owned(),
            labels: labels(worker, "outer-network", Some(worker)),
        },
    ];
    for (name, role) in [
        (worker.to_owned(), "socket"),
        (format!("{worker}-work"), "work"),
        (format!("{worker}-externals"), "externals"),
        (format!("{worker}-docker"), "dind-data"),
        (format!("{worker}-home"), "home-state"),
        (format!("{worker}-tmp"), "runner-temp"),
    ] {
        resources.push(OwnedDockerResource {
            kind: OwnedDockerResourceKind::Volume,
            id_or_name: name,
            names: Vec::new(),
            worker: worker.to_owned(),
            role: role.to_owned(),
            labels: labels(worker, role, None),
        });
    }
    Ok(resources)
}

fn resource(
    kind: OwnedDockerResourceKind,
    id_or_name: &str,
    worker: &str,
    role: &str,
    volume: &str,
) -> OwnedDockerResource {
    OwnedDockerResource {
        kind,
        id_or_name: id_or_name.to_owned(),
        names: Vec::new(),
        worker: worker.to_owned(),
        role: role.to_owned(),
        labels: labels(worker, role, Some(volume)),
    }
}

fn labels(worker: &str, role: &str, volume: Option<&str>) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::from([
        ("velnor.worker".to_owned(), worker.to_owned()),
        ("velnor.role".to_owned(), role.to_owned()),
    ]);
    if let Some(volume) = volume {
        labels.insert("velnor.volume".to_owned(), volume.to_owned());
    }
    labels
}
