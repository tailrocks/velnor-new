use super::super::cleanup::read_observer_witness;
use crate::error::HostError;
use crate::journal::connection::StorageObserver;
use turso::Connection;

#[cfg(test)]
#[path = "metadata_identity_tests.rs"]
mod tests;

struct View {
    observer: StorageObserver,
    target: String,
    container_id: Option<String>,
    read_only: bool,
    device: (i64, i64),
    statx_device: (i64, i64),
    fs_type: i64,
    fsid: String,
    digest: String,
    mount_root: String,
    inode: i64,
    subpath: Option<String>,
}

pub(super) async fn validate(
    connection: &Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let Some(witness) = read_observer_witness(connection, connection_id, reservation_id).await?
    else {
        return Ok(false);
    };
    let views = read_views(connection, reservation_id).await?;
    if views.len() != 6 {
        return Ok(false);
    }
    let root_digest = role_digest(connection, reservation_id).await?;
    validate_views(&views, &witness, &root_digest)
}

fn validate_views(
    views: &[View],
    witness: &super::super::cleanup::StorageCleanupObserverWitness,
    root_digest: &str,
) -> Result<bool, HostError> {
    if !validate_probe_view(views, root_digest)? || !validate_expected_views(views, witness) {
        return Ok(false);
    }
    let workers = views
        .iter()
        .filter(|view| view.observer != StorageObserver::Probe)
        .collect::<Vec<_>>();
    if workers.len() != 5 {
        return Ok(false);
    }
    if !validate_worker_group(&workers) {
        return Ok(false);
    }
    let Some(volume) = witness
        .binding()
        .volume_mountpoints
        .iter()
        .find(|volume| volume.role == "engine-metadata")
    else {
        return Ok(false);
    };
    if !initialized_job_facts_match(views, witness.metadata_job_facts_inode()) {
        return Ok(false);
    }
    if workers.iter().any(|view| {
        view.device
            != (
                i64::from(volume.mount_device[0]),
                i64::from(volume.mount_device[1]),
            )
            || view.statx_device
                != (
                    i64::from(volume.statx_device[0]),
                    i64::from(volume.statx_device[1]),
                )
    }) {
        return Ok(false);
    }
    validate_worker_subpaths(views)
}

fn validate_probe_view(views: &[View], root_digest: &str) -> Result<bool, HostError> {
    let probe = unique(views, StorageObserver::Probe, "/velnor/docker-root")?;
    Ok(probe.digest == root_digest && probe.subpath.is_none())
}

fn validate_expected_views(
    views: &[View],
    witness: &super::super::cleanup::StorageCleanupObserverWitness,
) -> bool {
    let expected = [
        (StorageObserver::Runner, "/.velnor-runtime-identity"),
        (StorageObserver::Runner, "/opt/velnor/job-facts"),
        (StorageObserver::Runner, "/opt/velnor/source-proof/control"),
        (StorageObserver::Dind, "/.velnor-runtime-identity"),
        (StorageObserver::Dind, "/opt/velnor/job-facts"),
    ];
    !expected.iter().any(|(observer, target)| {
        views
            .iter()
            .filter(|view| view.observer == *observer && view.target == *target)
            .count()
            != 1
    }) && !views.iter().any(|view| match view.observer {
        StorageObserver::Probe => view.container_id.is_some(),
        StorageObserver::Runner => {
            view.container_id.as_deref() != Some(witness.runner_container_id())
        }
        StorageObserver::Dind => view.container_id.as_deref() != Some(witness.dind_container_id()),
    })
}

fn validate_worker_group(workers: &[&View]) -> bool {
    let first = workers[0];
    let Some(root) = volume_root(first) else {
        return false;
    };
    !workers.iter().any(|view| {
        view.inode <= 0
            || view.device != first.device
            || view.statx_device != first.statx_device
            || view.fs_type != first.fs_type
            || view.fsid != first.fsid
            || view.digest != first.digest
            || volume_root(view) != Some(root)
    })
}

fn validate_worker_subpaths(views: &[View]) -> Result<bool, HostError> {
    for (target, subpath) in [
        ("/.velnor-runtime-identity", "runtime-identity"),
        ("/opt/velnor/job-facts", "job-facts"),
    ] {
        let runner = unique(views, StorageObserver::Runner, target)?;
        let dind = unique(views, StorageObserver::Dind, target)?;
        if runner.subpath.as_deref() != Some(subpath)
            || dind.subpath.as_deref() != Some(subpath)
            || runner.inode != dind.inode
            || runner.mount_root != dind.mount_root
            || runner.device != dind.device
            || runner.statx_device != dind.statx_device
            || runner.fs_type != dind.fs_type
            || runner.fsid != dind.fsid
            || runner.digest != dind.digest
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn initialized_job_facts_match(views: &[View], inode: Option<u64>) -> bool {
    let Some(inode) = inode.and_then(|value| i64::try_from(value).ok()) else {
        return false;
    };
    inode > 0
        && [StorageObserver::Runner, StorageObserver::Dind]
            .iter()
            .all(|observer| {
                unique(views, *observer, "/opt/velnor/job-facts")
                    .is_ok_and(|view| view.inode == inode)
            })
}

async fn read_views(connection: &Connection, reservation_id: i64) -> Result<Vec<View>, HostError> {
    let mut rows = connection
        .query(
            "SELECT a.observer, a.mount_target, a.container_id, a.read_only, a.device_major, a.device_minor, a.statx_device_major, a.statx_device_minor, a.fs_type_magic, a.filesystem_id, a.filesystem_instance_digest, p.mount_root, p.statx_inode, p.volume_subpath FROM capacity_reservation_bound_storage_attachments AS a JOIN capacity_reservation_bound_storage_paths AS p ON p.reservation_id = a.reservation_id AND p.role = a.role AND p.observer = a.observer AND p.mount_target = a.mount_target WHERE a.reservation_id = ?1 AND a.role = 'engine-metadata' ORDER BY a.observer, a.mount_target",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let observer = match row
            .get::<String>(0)
            .map_err(|_| HostError::Journal)?
            .as_str()
        {
            "probe" => StorageObserver::Probe,
            "runner" => StorageObserver::Runner,
            "dind" => StorageObserver::Dind,
            _ => return Err(HostError::Journal),
        };
        let fsid = row.get::<String>(9).map_err(|_| HostError::Journal)?;
        let inode = row.get::<Option<i64>>(12).map_err(|_| HostError::Journal)?;
        let view = View {
            observer,
            target: row.get(1).map_err(|_| HostError::Journal)?,
            container_id: row.get(2).map_err(|_| HostError::Journal)?,
            read_only: row.get::<i64>(3).map_err(|_| HostError::Journal)? == 1,
            device: (
                row.get(4).map_err(|_| HostError::Journal)?,
                row.get(5).map_err(|_| HostError::Journal)?,
            ),
            statx_device: (
                row.get(6).map_err(|_| HostError::Journal)?,
                row.get(7).map_err(|_| HostError::Journal)?,
            ),
            fs_type: row.get(8).map_err(|_| HostError::Journal)?,
            fsid,
            digest: row.get(10).map_err(|_| HostError::Journal)?,
            mount_root: row.get(11).map_err(|_| HostError::Journal)?,
            inode: inode.unwrap_or_default(),
            subpath: row.get(13).map_err(|_| HostError::Journal)?,
        };
        if !view.read_only || view.digest.len() != 64 || view.fsid.len() != 16 {
            return Ok(Vec::new());
        }
        result.push(view);
    }
    Ok(result)
}

async fn role_digest(connection: &Connection, reservation_id: i64) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT filesystem_instance_digest FROM capacity_reservation_bound_storage_roles WHERE reservation_id = ?1 AND role = 'engine-metadata'",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let digest = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(digest)
}

fn unique<'a>(
    views: &'a [View],
    observer: StorageObserver,
    target: &str,
) -> Result<&'a View, HostError> {
    let mut rows = views
        .iter()
        .filter(|view| view.observer == observer && view.target == target);
    let view = rows.next().ok_or(HostError::Identity)?;
    if rows.next().is_some() {
        return Err(HostError::Identity);
    }
    Ok(view)
}

fn volume_root(view: &View) -> Option<&str> {
    let suffix = format!("/{}", view.subpath.as_deref()?);
    view.mount_root
        .strip_suffix(&suffix)
        .filter(|root| !root.is_empty())
}
