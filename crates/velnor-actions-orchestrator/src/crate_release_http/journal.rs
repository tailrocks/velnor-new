// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GitHubAdmissionHttp, JOURNAL_BLOB_JSON_LIMIT, OrchestratorError, Value, encode, internal,
    journal_tag_hash, valid_sha, validate_journal_name, validate_journal_tag_response,
    validate_repository,
};

impl GitHubAdmissionHttp {
    pub(crate) fn check_journal_repository_access(
        &self,
        repository: &str,
    ) -> Result<(), OrchestratorError> {
        validate_repository(repository)?;
        self.verify_repository_access(&format!(
            "repos/{repository}/git/ref/tags/velnor-receipt%2F1%2F1%2Fpublication-started"
        ))
    }
    pub fn list_journal_operations(
        &self,
        repository: &str,
        run_id: u64,
        attempt: u32,
    ) -> Result<Vec<(String, String)>, OrchestratorError> {
        validate_repository(repository)?;
        if run_id == 0 || attempt == 0 {
            return Err(internal("crate_release_http_write_input"));
        }
        let prefix = format!("velnor-receipt/{run_id}/{attempt}/operation-");
        let route = format!(
            "repos/{repository}/git/matching-refs/tags/{}",
            encode(&prefix)
        );
        let body = self.get(&route)?;
        let entries = body
            .as_array()
            .filter(|items| items.len() <= 1000)
            .ok_or_else(|| internal("crate_release_http_response"))?;
        let mut unique = std::collections::BTreeMap::new();
        for entry in entries {
            let reference = entry["ref"]
                .as_str()
                .ok_or_else(|| internal("crate_release_http_response"))?;
            let name = reference
                .strip_prefix("refs/tags/")
                .ok_or_else(|| internal("crate_release_http_response"))?;
            if !name.starts_with(&prefix)
                || validate_journal_name(name).is_err()
                || entry["object"]["type"].as_str() != Some("tag")
                || !entry["object"]["sha"].as_str().is_some_and(valid_sha)
                || unique
                    .insert(
                        reference.to_owned(),
                        entry["object"]["sha"]
                            .as_str()
                            .ok_or_else(|| internal("crate_release_http_response"))?
                            .to_owned(),
                    )
                    .is_some()
            {
                return Err(internal("crate_release_http_response"));
            }
        }
        Ok(unique.into_iter().collect())
    }

    pub fn create_journal_tag(
        &self,
        repository: &str,
        tag: &str,
        blob_sha: &str,
    ) -> Result<String, OrchestratorError> {
        validate_repository(repository)?;
        let expected = journal_tag_hash(tag, blob_sha)?;
        let body = serde_json::json!({"tag":tag,"message":"velnor-release-receipt-v1","object":blob_sha,"type":"blob","tagger":{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"}});
        let response = self.post(&format!("repos/{repository}/git/tags"), &body)?;
        validate_journal_tag_response(&response, &expected)?;
        if response["tag"].as_str() != Some(tag)
            || response["object"]["sha"].as_str() != Some(blob_sha)
        {
            return Err(internal("crate_release_http_write_identity"));
        }
        Ok(expected)
    }
    pub fn create_journal_ref(
        &self,
        repository: &str,
        reference: &str,
        tag_sha: &str,
    ) -> Result<(), OrchestratorError> {
        validate_repository(repository)?;
        let name = reference
            .strip_prefix("refs/tags/")
            .ok_or_else(|| internal("crate_release_http_write_input"))?;
        validate_journal_name(name)?;
        if !valid_sha(tag_sha) {
            return Err(internal("crate_release_http_write_input"));
        }
        let response = self.post(
            &format!("repos/{repository}/git/refs"),
            &serde_json::json!({"ref":reference,"sha":tag_sha}),
        )?;
        if response["ref"].as_str() != Some(reference)
            || response["object"]["type"].as_str() != Some("tag")
            || response["object"]["sha"].as_str() != Some(tag_sha)
        {
            return Err(internal("crate_release_http_write_identity"));
        }
        Ok(())
    }
    pub fn get_journal_ref(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<Option<String>, OrchestratorError> {
        validate_repository(repository)?;
        let name = reference
            .strip_prefix("refs/tags/")
            .ok_or_else(|| internal("crate_release_http_write_input"))?;
        validate_journal_name(name)?;
        let response =
            self.get_optional(&format!("repos/{repository}/git/ref/tags/{}", encode(name)))?;
        response
            .map(|item| {
                let sha = item["object"]["sha"]
                    .as_str()
                    .filter(|sha| valid_sha(sha))
                    .ok_or_else(|| internal("crate_release_http_response"))?;
                if item["ref"].as_str() != Some(reference)
                    || item["object"]["type"].as_str() != Some("tag")
                {
                    return Err(internal("crate_release_http_response"));
                }
                Ok(sha.to_owned())
            })
            .transpose()
    }
    pub fn get_journal_tag(&self, repository: &str, sha: &str) -> Result<Value, OrchestratorError> {
        validate_repository(repository)?;
        if !valid_sha(sha) {
            return Err(internal("crate_release_http_write_input"));
        }
        let response = self.get(&format!("repos/{repository}/git/tags/{sha}"))?;
        validate_journal_tag_response(&response, sha)?;
        Ok(response)
    }
    pub fn get_journal_blob(
        &self,
        repository: &str,
        sha: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        use base64::Engine as _;
        validate_repository(repository)?;
        if !valid_sha(sha) {
            return Err(internal("crate_release_http_write_input"));
        }
        let response = self
            .request_with_limit(
                &format!("repos/{repository}/git/blobs/{sha}"),
                false,
                JOURNAL_BLOB_JSON_LIMIT,
            )?
            .ok_or_else(|| internal("crate_release_http_status"))?;
        let invalid = || internal("crate_release_http_response");
        if response["sha"].as_str() != Some(sha) || response["encoding"].as_str() != Some("base64")
        {
            return Err(invalid());
        }
        let encoded = response["content"]
            .as_str()
            .ok_or_else(invalid)?
            .replace('\n', "");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid())?;
        if bytes.len() > 32 * 1024 * 1024
            || response["size"].as_u64() != Some(bytes.len() as u64)
            || crate::release_policy_admission::git_blob_sha1(&bytes)? != sha
        {
            return Err(invalid());
        }
        Ok(bytes)
    }
    pub fn create_journal_blob(
        &self,
        repository: &str,
        bytes: &[u8],
    ) -> Result<String, OrchestratorError> {
        use base64::Engine as _;
        validate_repository(repository)?;
        if bytes.is_empty() || bytes.len() > 32 * 1024 * 1024 {
            return Err(internal("crate_release_journal_blob_input"));
        }
        let sha = crate::release_policy_admission::git_blob_sha1(bytes)?;
        let body = serde_json::json!({"content":base64::engine::general_purpose::STANDARD.encode(bytes),"encoding":"base64"});
        let response = self.post(&format!("repos/{repository}/git/blobs"), &body)?;
        if response["sha"].as_str() != Some(sha.as_str()) {
            return Err(internal("crate_release_http_write_identity"));
        }
        Ok(sha)
    }
}
