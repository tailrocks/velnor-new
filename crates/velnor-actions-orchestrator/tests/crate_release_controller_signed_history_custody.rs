// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;

pub(super) struct CustodyTransport<'a>(pub(super) &'a super::super::super::JournalRegistryFake);

impl crate::crate_release_journal_adapter::JournalTransport for CustodyTransport<'_> {
    fn check_repository_access(&self, repository: &str) -> Result<(), OrchestratorError> {
        crate::crate_release_journal::JournalIo::check_repository_access(self.0, repository)
    }
    fn check_tag_rules(&self, repository: &str) -> Result<(), OrchestratorError> {
        crate::crate_release_journal::JournalIo::check_tag_rules(self.0, repository)
    }
    fn get_ref(&self, repository: &str, name: &str) -> Result<Option<String>, OrchestratorError> {
        crate::crate_release_journal::JournalIo::get_ref(self.0, repository, name)
    }
    fn create_blob(&self, repository: &str, bytes: &[u8]) -> Result<String, OrchestratorError> {
        crate::crate_release_journal::JournalIo::create_blob(self.0, repository, bytes)
    }
    fn create_tag(
        &self,
        repository: &str,
        name: &str,
        blob: &str,
    ) -> Result<String, OrchestratorError> {
        crate::crate_release_journal::JournalIo::create_tag_object(
            self.0,
            repository,
            &crate::crate_release_journal::JournalTag {
                name: name.into(),
                object_type: "blob".into(),
                object_sha: blob.into(),
                message: "velnor-release-receipt-v1".into(),
            },
        )
    }
    fn create_ref(
        &self,
        repository: &str,
        name: &str,
        target: &str,
    ) -> Result<(), OrchestratorError> {
        crate::crate_release_journal::JournalIo::create_ref(self.0, repository, name, target)
    }
    fn get_tag(
        &self,
        repository: &str,
        sha: &str,
    ) -> Result<crate::crate_release_journal_adapter::RemoteTag, OrchestratorError> {
        Ok(crate::crate_release_journal_adapter::RemoteTag {
            sha: sha.into(),
            tag: crate::crate_release_journal::JournalIo::get_tag_object(self.0, repository, sha)?,
            tagger_name: "Velnor Release".into(),
            tagger_email: "release@velnor.invalid".into(),
            tagger_date: "1970-01-01T00:00:00Z".into(),
            unsigned: true,
        })
    }
    fn get_blob(&self, repository: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        crate::crate_release_journal::JournalIo::get_blob(self.0, repository, sha)
    }
}

impl crate::crate_release_journal_adapter::JournalRefTransport for CustodyTransport<'_> {
    fn list_operations(
        &self,
        identity: &crate::crate_release_journal::JournalIdentity,
    ) -> Result<Vec<(String, String)>, OrchestratorError> {
        crate::crate_release_journal::JournalOperationRefs::list_operation_refs(self.0, identity)
    }
}
