#[path = "owned_deletion.rs"]
mod owned_deletion;

pub(super) use owned_deletion::{
    ContainerDeletion, remove_container, remove_network, remove_owned_volumes,
};
