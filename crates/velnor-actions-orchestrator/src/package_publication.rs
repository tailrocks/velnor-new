use crate::{OrchestratorError, internal::internal};
use std::collections::BTreeSet;
use velnor_actions_contract::publication_retry::{
    PublicationAction, PublicationIdentity, PublicationState, publication_action,
};

pub trait PackagePublicationBackend {
    fn authenticate_original(&mut self) -> Result<PublicationIdentity, OrchestratorError>;
    fn observe(&mut self) -> Result<PublicationState, OrchestratorError>;
    fn available_original_assets(&mut self) -> Result<BTreeSet<String>, OrchestratorError>;
    fn candidate_current(&mut self) -> Result<bool, OrchestratorError>;
    fn create_tag(&mut self) -> Result<(), OrchestratorError>;
    fn create_draft(&mut self) -> Result<(), OrchestratorError>;
    fn upload_original_asset(&mut self, name: &str) -> Result<(), OrchestratorError>;
    fn publish_draft(&mut self) -> Result<(), OrchestratorError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackagePublicationOutcome {
    Published,
    AlreadyPublished,
}

pub fn publish_package_release(
    backend: &mut impl PackagePublicationBackend,
) -> Result<PackagePublicationOutcome, OrchestratorError> {
    let expected = backend.authenticate_original()?;
    if expected.assets.is_empty() {
        return Err(internal("package_publication_empty_inventory"));
    }
    let mut published_here = false;
    let max_steps = expected.assets.len().saturating_add(4);
    for _ in 0..max_steps {
        if backend.authenticate_original()? != expected {
            return Err(internal("package_publication_original_changed"));
        }
        let observed = backend.observe()?;
        let available = backend.available_original_assets()?;
        let current = true;
        match publication_action(&expected, &observed, current, &available) {
            PublicationAction::CreateTag => {
                backend.create_tag()?;
            }
            PublicationAction::CreateDraft => {
                backend.create_draft()?;
            }
            PublicationAction::CompleteDraft { missing_assets } => {
                if let Some(name) = missing_assets.first() {
                    backend.upload_original_asset(name)?;
                } else {
                    backend.publish_draft()?;
                    published_here = true;
                }
            }
            PublicationAction::ReconcileDistribution => {
                return Ok(if published_here {
                    PackagePublicationOutcome::Published
                } else {
                    PackagePublicationOutcome::AlreadyPublished
                });
            }
            PublicationAction::Reject(reason) => {
                return Err(internal(&format!("package_publication_refused:{reason:?}")));
            }
        }
    }
    Err(internal("package_publication_postcondition_not_reached"))
}
