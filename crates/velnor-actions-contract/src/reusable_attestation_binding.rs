// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ContractError;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRevision {
    pub repository: String,
    pub path: String,
    pub git_ref: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestedSource {
    pub repository: String,
    pub repository_id: u64,
    pub owner_id: u64,
    pub git_ref: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReusableAttestationBinding {
    pub signer: WorkflowRevision,
    pub caller: WorkflowRevision,
    pub source: AttestedSource,
    pub event: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub subjects: std::collections::BTreeMap<String, String>,
}

impl ReusableAttestationBinding {
    pub fn check_untrusted_output_shape(&self, bytes: &[u8]) -> Result<(), ContractError> {
        self.validate()?;
        let output = crate::strict_json::parse_strict_json_bytes(bytes, 128 * 1024)?;
        let [entry] = output.as_array().ok_or_else(invalid)?.as_slice() else {
            return Err(invalid());
        };
        let result = &entry["verificationResult"];
        self.check_certificate(&result["signature"]["certificate"])?;
        self.check_statement(&result["statement"])
    }

    fn validate(&self) -> Result<(), ContractError> {
        for workflow in [&self.signer, &self.caller] {
            let path = workflow.path.strip_prefix(".github/workflows/");
            if !crate::vcs::is_safe_github_repository(&workflow.repository)
                || !valid_ref(&workflow.git_ref)
                || !hex(&workflow.revision, 40)
                || !path.is_some_and(|p| {
                    !p.is_empty()
                        && !p.starts_with('.')
                        && p.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
                        && p.rsplit_once('.')
                            .is_some_and(|(_, extension)| matches!(extension, "yml" | "yaml"))
                })
            {
                return Err(invalid());
            }
        }
        if !crate::vcs::is_safe_github_repository(&self.source.repository)
            || self.caller.repository != self.source.repository
            || !valid_ref(&self.source.git_ref)
            || !hex(&self.source.revision, 40)
            || self.source.repository_id == 0
            || self.source.owner_id == 0
            || self.run_id == 0
            || self.run_attempt == 0
            || !matches!(
                self.event.as_str(),
                "pull_request" | "push" | "workflow_dispatch"
            )
            || self.subjects.is_empty()
            || self.subjects.len() > 32
            || self.subjects.iter().any(|(name, digest)| {
                name.is_empty()
                    || name.len() > 255
                    || name
                        .bytes()
                        .any(|b| !b.is_ascii_graphic() || matches!(b, b'/' | b'\\'))
                    || !hex(digest, 64)
            })
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn check_certificate(&self, certificate: &Value) -> Result<(), ContractError> {
        let source_uri = format!("https://github.com/{}", self.source.repository);
        let owner = self
            .source
            .repository
            .split_once('/')
            .ok_or_else(invalid)?
            .0;
        let values = [
            (
                "issuer",
                "https://token.actions.githubusercontent.com".to_owned(),
            ),
            ("runnerEnvironment", "github-hosted".to_owned()),
            ("subjectAlternativeName", workflow_uri(&self.signer)),
            ("buildSignerURI", workflow_uri(&self.signer)),
            ("buildSignerDigest", self.signer.revision.clone()),
            ("buildConfigURI", workflow_uri(&self.caller)),
            ("buildConfigDigest", self.caller.revision.clone()),
            ("sourceRepositoryURI", source_uri),
            ("sourceRepositoryDigest", self.source.revision.clone()),
            ("sourceRepositoryRef", self.source.git_ref.clone()),
            (
                "sourceRepositoryIdentifier",
                self.source.repository_id.to_string(),
            ),
            (
                "sourceRepositoryOwnerURI",
                format!("https://github.com/{owner}"),
            ),
            (
                "sourceRepositoryOwnerIdentifier",
                self.source.owner_id.to_string(),
            ),
            ("buildTrigger", self.event.clone()),
            ("runInvocationURI", self.invocation_uri()),
        ];
        if values
            .iter()
            .any(|(key, expected)| certificate[*key].as_str() != Some(expected))
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn check_statement(&self, statement: &Value) -> Result<(), ContractError> {
        self.check_subjects(&statement["subject"])?;
        let caller = serde_json::json!({"repository":format!("https://github.com/{}",self.caller.repository),"path":self.caller.path,"ref":self.caller.git_ref});
        let dependencies = serde_json::json!([{"uri":format!("git+https://github.com/{}@{}",self.source.repository,self.source.git_ref),"digest":{"gitCommit":self.source.revision}}]);
        let predicate = &statement["predicate"];
        let github = &predicate["buildDefinition"]["internalParameters"]["github"];
        if statement["_type"] != "https://in-toto.io/Statement/v1"
            || statement["predicateType"] != "https://slsa.dev/provenance/v1"
            || predicate["buildDefinition"]["buildType"]
                != "https://actions.github.io/buildtypes/workflow/v1"
            || predicate["buildDefinition"]["externalParameters"]["workflow"] != caller
            || predicate["buildDefinition"]["resolvedDependencies"] != dependencies
            || github["event_name"] != self.event
            || !number_matches(&github["repository_id"], self.source.repository_id)
            || !number_matches(&github["repository_owner_id"], self.source.owner_id)
            || github["runner_environment"] != "github-hosted"
            || predicate["runDetails"]["builder"]["id"] != workflow_uri(&self.signer)
            || predicate["runDetails"]["metadata"]["invocationId"] != self.invocation_uri()
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn check_subjects(&self, subjects: &Value) -> Result<(), ContractError> {
        let subjects = subjects.as_array().ok_or_else(invalid)?;
        if subjects.len() != self.subjects.len() {
            return Err(invalid());
        }
        let mut observed = std::collections::BTreeMap::new();
        for subject in subjects {
            let name = subject["name"].as_str().ok_or_else(invalid)?;
            let digest = subject["digest"]["sha256"].as_str().ok_or_else(invalid)?;
            if subject != &serde_json::json!({"name": name, "digest": {"sha256": digest}})
                || observed
                    .insert(name.to_owned(), digest.to_owned())
                    .is_some()
            {
                return Err(invalid());
            }
        }
        if observed != self.subjects {
            return Err(invalid());
        }
        Ok(())
    }

    fn invocation_uri(&self) -> String {
        format!(
            "https://github.com/{}/actions/runs/{}/attempts/{}",
            self.source.repository, self.run_id, self.run_attempt
        )
    }
}

fn workflow_uri(workflow: &WorkflowRevision) -> String {
    format!(
        "https://github.com/{}/{}@{}",
        workflow.repository, workflow.path, workflow.git_ref
    )
}

fn valid_ref(value: &str) -> bool {
    value
        .strip_prefix("refs/")
        .is_some_and(crate::is_valid_branch_name)
}

fn hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn invalid() -> ContractError {
    ContractError::identity("reusable_attestation_binding", "identity_mismatch")
}

fn number_matches(value: &Value, expected: u64) -> bool {
    value.as_str().is_some_and(|s| {
        !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u64>() == Ok(expected)
    })
}
