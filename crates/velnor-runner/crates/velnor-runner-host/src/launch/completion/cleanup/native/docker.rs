#[path = "owned_deletion.rs"]
mod owned_deletion;

pub(super) use owned_deletion::{
    ContainerDeletion, remove_container, remove_network, remove_owned_volumes,
};
pub(in crate::launch) use owned_deletion::{
    remove_owned_resources_for_jit_conflict, remove_owned_resources_for_retirement,
};
