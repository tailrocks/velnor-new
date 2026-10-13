//! Complete read-only inventory of daemon objects carrying Velnor labels.

use std::collections::{BTreeMap, HashMap, HashSet};

use tokio::time::Instant;

use crate::HostError;

mod response;
mod transport;
use response::{
    BoundedMap, BoundedVec, BoundedVolumeListResponse, InventoryContainer, InventoryNetwork,
    InventoryVolume,
};
use transport::{BoundedDockerApi, acquire_inventory_slot, ensure_before_deadline};

/// Maximum accepted response objects per Docker list endpoint.
///
/// The custom decoder rejects the first item above this limit before growing
/// the output vector. It materializes only required identity fields, bounds
/// names and label maps during deserialization, and ignores unused Docker API
/// fields without allocating their nested collections.
pub const MAX_INVENTORY_OBJECTS_PER_KIND: usize = 16_384;
pub use transport::MAX_INVENTORY_RESPONSE_BYTES;

const MAX_LABEL_COUNT: usize = 64;
const MAX_LABEL_KEY_BYTES: usize = 128;
const MAX_LABEL_VALUE_BYTES: usize = 4096;
const MAX_LABEL_TOTAL_BYTES: usize = 8192;
const MAX_RESOURCE_ID_BYTES: usize = 256;
const MAX_RESOURCE_NAME_BYTES: usize = 255;
const MAX_CONTAINER_NAMES: usize = 16;

/// Docker object class in a Velnor ownership inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnedDockerResourceKind {
    /// Runner or private `DinD` container.
    Container,
    /// Per-generation outer network.
    Network,
    /// Worker, socket, work, externals, home, temp, or `DinD` data volume.
    Volume,
}

/// Exact identity and ownership labels for one Velnor Docker object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedDockerResource {
    /// Object type.
    pub kind: OwnedDockerResourceKind,
    /// Container/network ID, or volume name (the volume API has no ID field).
    pub id_or_name: String,
    /// Exact Docker API names. Container names retain their leading slash.
    pub names: Vec<String>,
    /// Required `velnor.worker` label.
    pub worker: String,
    /// Required `velnor.role` label; unknown future roles remain visible.
    pub role: String,
    /// All labels exactly as returned, in key order for stable comparisons.
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug)]
struct OwnerLabels {
    worker: String,
    role: String,
    labels: BTreeMap<String, String>,
}

/// Return every Docker container, network, and volume carrying a Velnor label.
///
/// The list endpoints are unfiltered and have no cursor/pagination contract.
/// Objects are selected locally only after each complete endpoint response is
/// received. Every body is streamed under the caller's absolute deadline and
/// rejected above [`MAX_INVENTORY_RESPONSE_BYTES`] before JSON decode. The
/// decoder caps top-level and nested collection sizes, materializing only
/// identity, name, and label fields. Any endpoint error, timeout,
/// incomplete volume response or warning, malformed Velnor label, duplicate
/// identity, or over-limit list makes the whole call fail; no partial vector
/// is returned.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the daemon call times out/fails or the
/// endpoint is invalid or the returned ownership inventory is incomplete,
/// malformed, ambiguous, too large, or late.
pub async fn list_owned_docker_resources_until(
    endpoint: &str,
    deadline: Instant,
) -> Result<Vec<OwnedDockerResource>, HostError> {
    let _inventory_permit = acquire_inventory_slot(deadline).await?;
    ensure_before_deadline(deadline)?;
    let api = BoundedDockerApi::new(endpoint)?;
    let mut inventory = InventoryBuilder::default();
    let containers: BoundedVec<InventoryContainer, MAX_INVENTORY_OBJECTS_PER_KIND> =
        api.get_json("/containers/json?all=1", deadline).await?;
    inventory.add_containers(containers.into_vec())?;
    ensure_before_deadline(deadline)?;

    let networks: BoundedVec<InventoryNetwork, MAX_INVENTORY_OBJECTS_PER_KIND> =
        api.get_json("/networks", deadline).await?;
    inventory.add_networks(networks.into_vec())?;
    ensure_before_deadline(deadline)?;

    let volumes: BoundedVolumeListResponse = api.get_json("/volumes", deadline).await?;
    inventory.add_volumes(volumes.into_volumes()?)?;
    let inventory = finish_inventory_until(deadline, || inventory.finish())?;
    Ok(inventory)
}

fn finish_inventory_until<T>(
    deadline: Instant,
    finish: impl FnOnce() -> T,
) -> Result<T, HostError> {
    ensure_before_deadline(deadline)?;
    let result = finish();
    ensure_before_deadline(deadline)?;
    Ok(result)
}

#[cfg(test)]
fn inventory_from_lists(
    containers: Vec<InventoryContainer>,
    networks: Vec<InventoryNetwork>,
    volumes: BoundedVolumeListResponse,
) -> Result<Vec<OwnedDockerResource>, HostError> {
    let mut inventory = InventoryBuilder::default();
    inventory.add_containers(containers)?;
    inventory.add_networks(networks)?;
    inventory.add_volumes(volumes.into_volumes()?)?;
    Ok(inventory.finish())
}

#[derive(Default)]
struct InventoryBuilder {
    resources: Vec<OwnedDockerResource>,
    seen_ids: HashSet<(OwnedDockerResourceKind, String)>,
    seen_names: HashSet<(OwnedDockerResourceKind, String)>,
}

impl InventoryBuilder {
    fn add_containers(&mut self, containers: Vec<InventoryContainer>) -> Result<(), HostError> {
        if containers.len() > MAX_INVENTORY_OBJECTS_PER_KIND {
            return Err(HostError::Docker);
        }
        for container in containers {
            let Some(owner) = owner_labels(container.labels.map(BoundedMap::into_map))? else {
                continue;
            };
            let id = bounded_required(container.id, MAX_RESOURCE_ID_BYTES)?;
            let names = bounded_names(container.names.ok_or(HostError::Docker)?.into_vec())?;
            self.push(OwnedDockerResourceKind::Container, id, names, owner)?;
        }
        Ok(())
    }

    fn add_networks(&mut self, networks: Vec<InventoryNetwork>) -> Result<(), HostError> {
        if networks.len() > MAX_INVENTORY_OBJECTS_PER_KIND {
            return Err(HostError::Docker);
        }
        for network in networks {
            let Some(owner) = owner_labels(network.labels.map(BoundedMap::into_map))? else {
                continue;
            };
            let id = bounded_required(network.id, MAX_RESOURCE_ID_BYTES)?;
            let name = bounded_required(network.name, MAX_RESOURCE_NAME_BYTES)?;
            self.push(OwnedDockerResourceKind::Network, id, vec![name], owner)?;
        }
        Ok(())
    }

    fn add_volumes(&mut self, volumes: Vec<InventoryVolume>) -> Result<(), HostError> {
        if volumes.len() > MAX_INVENTORY_OBJECTS_PER_KIND {
            return Err(HostError::Docker);
        }
        for volume in volumes {
            let Some(owner) = owner_labels(volume.labels.map(BoundedMap::into_map))? else {
                continue;
            };
            let name = bounded_required(volume.name, MAX_RESOURCE_NAME_BYTES)?;
            self.push(
                OwnedDockerResourceKind::Volume,
                name.clone(),
                vec![name],
                owner,
            )?;
        }
        Ok(())
    }

    fn push(
        &mut self,
        kind: OwnedDockerResourceKind,
        id_or_name: String,
        names: Vec<String>,
        owner: OwnerLabels,
    ) -> Result<(), HostError> {
        push_owned(
            &mut self.resources,
            &mut self.seen_ids,
            &mut self.seen_names,
            kind,
            id_or_name,
            names,
            owner,
        )
    }

    fn finish(mut self) -> Vec<OwnedDockerResource> {
        self.resources.sort_by(|left, right| {
            (left.kind, &left.id_or_name).cmp(&(right.kind, &right.id_or_name))
        });
        self.resources
    }
}

fn owner_labels(labels: Option<HashMap<String, String>>) -> Result<Option<OwnerLabels>, HostError> {
    let labels = labels.unwrap_or_default();
    if !labels.keys().any(|key| is_velnor_key(key)) {
        return Ok(None);
    }
    if labels.len() > MAX_LABEL_COUNT {
        return Err(HostError::Docker);
    }
    let mut total_bytes = 0_usize;
    for (key, value) in &labels {
        total_bytes = total_bytes
            .checked_add(key.len())
            .and_then(|size| size.checked_add(value.len()))
            .ok_or(HostError::Docker)?;
        if total_bytes > MAX_LABEL_TOTAL_BYTES
            || key.len() > MAX_LABEL_KEY_BYTES
            || value.len() > MAX_LABEL_VALUE_BYTES
        {
            return Err(HostError::Docker);
        }
        if is_velnor_key(key)
            && (!valid_velnor_key(key)
                || value.is_empty()
                || value.bytes().any(|byte| byte.is_ascii_control()))
        {
            return Err(HostError::Docker);
        }
    }
    let worker = labels
        .get("velnor.worker")
        .filter(|value| valid_worker_label(value))
        .cloned()
        .ok_or(HostError::Docker)?;
    let role = labels
        .get("velnor.role")
        .filter(|value| valid_role_label(value))
        .cloned()
        .ok_or(HostError::Docker)?;
    if labels
        .get("velnor.volume")
        .is_some_and(|volume| volume != &worker)
    {
        return Err(HostError::Docker);
    }
    Ok(Some(OwnerLabels {
        worker,
        role,
        labels: labels.into_iter().collect(),
    }))
}

fn is_velnor_key(key: &str) -> bool {
    key == "velnor" || key.starts_with("velnor.")
}

fn valid_velnor_key(key: &str) -> bool {
    key.strip_prefix("velnor.").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })
    })
}

fn valid_worker_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

fn valid_role_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

fn bounded_required(value: Option<String>, max_bytes: usize) -> Result<String, HostError> {
    let value = value.ok_or(HostError::Docker)?;
    if value.is_empty() || value.len() > max_bytes || value.bytes().any(|b| b.is_ascii_control()) {
        return Err(HostError::Docker);
    }
    Ok(value)
}

fn bounded_names(mut names: Vec<String>) -> Result<Vec<String>, HostError> {
    if names.is_empty() || names.len() > MAX_CONTAINER_NAMES {
        return Err(HostError::Docker);
    }
    let mut unique = HashSet::with_capacity(names.len());
    for name in &names {
        if name.is_empty()
            || name.len() > MAX_RESOURCE_NAME_BYTES
            || name.bytes().any(|byte| byte.is_ascii_control())
            || !unique.insert(name.as_str())
        {
            return Err(HostError::Docker);
        }
    }
    names.sort();
    Ok(names)
}

fn push_owned(
    resources: &mut Vec<OwnedDockerResource>,
    seen_ids: &mut HashSet<(OwnedDockerResourceKind, String)>,
    seen_names: &mut HashSet<(OwnedDockerResourceKind, String)>,
    kind: OwnedDockerResourceKind,
    id_or_name: String,
    names: Vec<String>,
    owner: OwnerLabels,
) -> Result<(), HostError> {
    if !seen_ids.insert((kind, id_or_name.clone())) {
        return Err(HostError::Docker);
    }
    for name in &names {
        if !seen_names.insert((kind, name.clone())) {
            return Err(HostError::Docker);
        }
    }
    resources.push(OwnedDockerResource {
        kind,
        id_or_name,
        names,
        worker: owner.worker,
        role: owner.role,
        labels: owner.labels,
    });
    Ok(())
}

#[cfg(test)]
#[path = "inventory/tests.rs"]
mod tests;
