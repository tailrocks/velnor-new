//! Mutation routes for already-admitted rolling transitions.

use super::super::{
    github::{GitHubApi, encode_segment, sha256},
    identity::read_regular,
    rolling_state::Envelope,
};
use crate::{OrchestratorError, internal::internal};
use std::path::Path;
use velnor_actions_contract::publication_retry::{
    PublicationIdentity,
    rolling::{RollingAction, staged_name},
};

pub(super) fn write(
    api: &GitHubApi,
    id: u64,
    envelope: &Envelope,
) -> Result<(), OrchestratorError> {
    if id == 0 || envelope.release_id != id {
        return Err(internal("rolling_write_release_id"));
    }
    api.patch_fields(
        &api.route(&format!("releases/{id}")),
        &[("body", envelope.body()?)],
    )?;
    Ok(())
}

pub(super) fn mutate(
    api: &GitHubApi,
    id: u64,
    identity: &PublicationIdentity,
    directory: &Path,
    action: RollingAction,
) -> Result<(), OrchestratorError> {
    if id == 0 {
        return Err(internal("rolling_mutation_release_id"));
    }
    match action {
        RollingAction::UploadStage(name) => upload(api, id, identity, directory, &name),
        RollingAction::DeleteStarter(id) | RollingAction::DeleteObsolete(id) => {
            api.delete_rolling_asset(id)
        }
        RollingAction::Rename { id, name } => {
            if id == 0 || !identity.assets.contains_key(&name) {
                return Err(internal("rolling_rename_identity"));
            }
            api.patch_fields(
                &api.route(&format!("releases/assets/{id}")),
                &[("name", name)],
            )?;
            Ok(())
        }
        RollingAction::AdvanceTag => {
            api.patch_fields(
                &api.route("git/refs/tags/preview"),
                &[
                    ("sha", identity.source_sha.clone()),
                    ("force", "false".to_owned()),
                ],
            )?;
            Ok(())
        }
        _ => Err(internal("rolling_nontransport_action")),
    }
}

fn upload(
    api: &GitHubApi,
    id: u64,
    identity: &PublicationIdentity,
    directory: &Path,
    name: &str,
) -> Result<(), OrchestratorError> {
    let digest = identity
        .assets
        .get(name)
        .ok_or_else(|| internal("rolling_upload_name"))?;
    let path = directory.join(name);
    if sha256(&read_regular(&path)?) != *digest {
        return Err(internal("rolling_upload_bytes_changed"));
    }
    api.upload_file(
        &format!(
            "{}?name={}",
            api.route(&format!("releases/{id}/assets")),
            encode_segment(&staged_name(identity, name))
        ),
        &path,
    )
}

#[cfg(test)]
#[path = "package_release_rolling_backend_transport_tests.rs"]
mod tests;
