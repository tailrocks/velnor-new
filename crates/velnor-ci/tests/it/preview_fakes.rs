// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use velnor_ci::error::CiError;
use velnor_ci::preview::attest::{Attestor, Authority, Subject};
use velnor_ci::preview::json::Res;
use velnor_ci::preview::net::Net;

use crate::preview_world::{AUTHORITY, REPO, SIGNER, World};

/// The fake network: answers from the world, fails what it does not know, and records
/// every API path asked.
#[derive(Debug)]
pub(crate) struct FakeNet {
    pub world: World,
    pub calls: RefCell<Vec<String>>,
    deadline: Instant,
}

impl FakeNet {
    pub(crate) fn new(world: World) -> Self {
        Self {
            world,
            calls: RefCell::new(Vec::new()),
            deadline: Instant::now() + Duration::from_secs(180),
        }
    }
}

impl Net for FakeNet {
    fn api(&self, path: &str) -> Res<Value> {
        self.calls.borrow_mut().push(path.to_owned());
        self.world
            .api
            .get(path)
            .cloned()
            .ok_or_else(|| CiError::new(format!("fake: no {path}")))
    }

    fn asset(&self, url: &str, maximum: usize) -> Res<Vec<u8>> {
        self.calls.borrow_mut().push(url.to_owned());
        let body = self
            .world
            .assets
            .get(url)
            .cloned()
            .ok_or_else(|| CiError::new(format!("fake: no {url}")))?;
        if body.len() > maximum {
            return Err(CiError::new("fake: too large"));
        }
        Ok(body)
    }

    fn deadline(&self) -> Instant {
        self.deadline
    }
}

/// Answers every attestation with the world's `gh` output, after the checks `gh`
/// itself would not make.
#[derive(Debug)]
pub(crate) struct FakeAttestor {
    pub output: Value,
    pub seen: RefCell<Vec<(Subject, Authority)>>,
}

impl Attestor for FakeAttestor {
    fn verify(
        &self,
        _receipt: &[u8],
        _bundle: &[u8],
        subject: &Subject,
        authority: &Authority,
        _deadline: Instant,
    ) -> Res<Value> {
        self.seen
            .borrow_mut()
            .push((subject.clone(), authority.clone()));
        velnor_ci::preview::attest::check_verified(&self.output, subject, authority)?;
        Ok(self.output.clone())
    }
}

/// What `gh attestation verify --format json` prints for a receipt the publication
/// workflow signed. Written out field by field, apart from the code under test.
pub(crate) fn attestation(record: &Value) -> Value {
    let text = |key: &str| record[key].as_str().unwrap_or_default().to_owned();
    let signer = format!(
        "https://github.com/{REPO}/.github/workflows/source-preview-publication.yml@{AUTHORITY}"
    );
    let source = text("source_sha");
    let tag_ref = format!("refs/tags/sdk-preview-{source}");
    json!([{"verificationResult": {
        "signature": {"certificate": {
            "issuer": "https://token.actions.githubusercontent.com",
            "subjectAlternativeName": signer, "buildSignerURI": signer, "buildSignerDigest": AUTHORITY,
            "buildConfigURI": format!("https://github.com/{REPO}/{SIGNER}@{tag_ref}"),
            "buildConfigDigest": source,
            "sourceRepositoryURI": format!("https://github.com/{REPO}"),
            "sourceRepositoryIdentifier": "1390620900",
            "sourceRepositoryOwnerURI": "https://github.com/tailrocks",
            "sourceRepositoryOwnerIdentifier": "78806509",
            "sourceRepositoryDigest": source, "sourceRepositoryRef": tag_ref,
            "buildTrigger": "push", "runnerEnvironment": "github-hosted",
            "runInvocationURI": format!("https://github.com/{REPO}/actions/runs/500/attempts/3"),
        }},
        "statement": {"predicateType": "https://slsa.dev/provenance/v1", "subject": [
            {"name": "SDK_PREVIEW_RECEIPT.json", "digest": {"sha256": text("receipt_sha256")}},
            {"name": text("linux_payload_name"), "digest": {"sha256": text("linux_payload_sha256")}},
            {"name": text("darwin_payload_name"), "digest": {"sha256": text("darwin_payload_sha256")}},
        ]},
    }}])
}
