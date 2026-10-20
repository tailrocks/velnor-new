use crate::{
    journal::{ConnectionJournal, Journal},
    launch::{ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages},
};

/// Shared host resources for publishing listener authorization proofs.
#[derive(Clone, Copy)]
pub(crate) struct ScopedHostContext<'a> {
    pub(crate) docker: &'a bollard::Docker,
    pub(crate) host: &'a Journal,
    pub(crate) scoped: &'a ConnectionJournal,
    pub(crate) secret: &'a ReleaseReadSecret,
    pub(crate) engine: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
}
