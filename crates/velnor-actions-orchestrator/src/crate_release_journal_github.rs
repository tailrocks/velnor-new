// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    OrchestratorError,
    crate_release_github::{NativeGitHubAdmissionRead, verify_immutable_tag_rules},
    crate_release_journal::JournalTag,
    crate_release_journal_adapter::{JournalTransport, RemoteTag},
    internal::internal,
};

impl JournalTransport for NativeGitHubAdmissionRead {
    fn check_repository_access(&self, repository: &str) -> Result<(), OrchestratorError> {
        self.reader.check_journal_repository_access(repository)
    }

    fn check_tag_rules(&self, repository: &str) -> Result<(), OrchestratorError> {
        verify_immutable_tag_rules(repository, self)
    }

    fn get_ref(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<Option<String>, OrchestratorError> {
        self.reader.get_journal_ref(repository, reference)
    }

    fn create_blob(&self, repository: &str, bytes: &[u8]) -> Result<String, OrchestratorError> {
        self.reader.create_journal_blob(repository, bytes)
    }

    fn create_tag(
        &self,
        repository: &str,
        name: &str,
        blob_sha: &str,
    ) -> Result<String, OrchestratorError> {
        self.reader.create_journal_tag(repository, name, blob_sha)
    }

    fn create_ref(
        &self,
        repository: &str,
        reference: &str,
        tag_sha: &str,
    ) -> Result<(), OrchestratorError> {
        self.reader
            .create_journal_ref(repository, reference, tag_sha)
    }

    fn get_tag(&self, repository: &str, sha: &str) -> Result<RemoteTag, OrchestratorError> {
        let response = self.reader.get_journal_tag(repository, sha)?;
        let missing = || internal("crate_release_journal_github_response");
        let field =
            |value: &serde_json::Value| value.as_str().map(str::to_owned).ok_or_else(missing);
        Ok(RemoteTag {
            sha: field(&response["sha"])?,
            tag: JournalTag {
                name: field(&response["tag"])?,
                object_type: field(&response["object"]["type"])?,
                object_sha: field(&response["object"]["sha"])?,
                message: field(&response["message"])?,
            },
            tagger_name: field(&response["tagger"]["name"])?,
            tagger_email: field(&response["tagger"]["email"])?,
            tagger_date: field(&response["tagger"]["date"])?,
            unsigned: response["verification"]["reason"].as_str() == Some("unsigned")
                && response["verification"]["verified"].as_bool() == Some(false)
                && response["verification"]["signature"].is_null()
                && response["verification"]["payload"].is_null(),
        })
    }

    fn get_blob(&self, repository: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        self.reader.get_journal_blob(repository, sha)
    }
}

impl crate::crate_release_journal_adapter::JournalRefTransport for NativeGitHubAdmissionRead {
    fn list_operations(
        &self,
        identity: &crate::crate_release_journal::JournalIdentity,
    ) -> Result<Vec<(String, String)>, OrchestratorError> {
        self.reader.list_journal_operations(
            &identity.repository,
            identity.run_id,
            identity.run_attempt,
        )
    }
}
