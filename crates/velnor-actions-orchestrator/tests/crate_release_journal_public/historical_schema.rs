// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn journal_migration_keeps_v1_and_rejects_forged_v2_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let v1 = receipt("0.1.1");
    let original = serde_json::to_value(&v1)?;
    assert!(original["packages"][0].get("tag_target").is_none());
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<JournalReceipt>(original.clone())?)?,
        original
    );
    let remote_v1 = Registry::default();
    record_journal(&remote_v1, &identity(), JournalStage::Started, &v1, None)?;
    record_journal(
        &remote_v1,
        &identity(),
        JournalStage::Completed,
        &v1,
        Some(&completion("0.1.1")),
    )?;
    let remote_v2 = Registry::default();
    let mut v2 = v1;
    v2.packages[0].tag_target = Some("f".repeat(40));
    record_journal(&remote_v2, &identity(), JournalStage::Started, &v2, None)?;
    let mut forged = completion("0.1.1");
    assert!(
        record_journal(
            &remote_v2,
            &identity(),
            JournalStage::Completed,
            &v2,
            Some(&forged)
        )
        .is_err()
    );
    forged.packages[0].tag_target = "f".repeat(40);
    record_journal(
        &remote_v2,
        &identity(),
        JournalStage::Completed,
        &v2,
        Some(&forged),
    )?;
    Ok(())
}

struct CustodyTransport<'a> {
    store: &'a Registry,
    unsigned: bool,
    deny_rules: bool,
}
impl crate::crate_release_journal_adapter::JournalTransport for CustodyTransport<'_> {
    fn check_repository_access(&self, repository: &str) -> Result<(), OrchestratorError> {
        self.store.check_repository_access(repository)
    }
    fn check_tag_rules(&self, repository: &str) -> Result<(), OrchestratorError> {
        if self.deny_rules {
            return Err(err());
        }
        self.store.check_tag_rules(repository)
    }
    fn get_ref(&self, repository: &str, name: &str) -> Result<Option<String>, OrchestratorError> {
        self.store.get_ref(repository, name)
    }
    fn create_blob(&self, repository: &str, bytes: &[u8]) -> Result<String, OrchestratorError> {
        self.store.create_blob(repository, bytes)
    }
    fn create_tag(
        &self,
        repository: &str,
        name: &str,
        blob: &str,
    ) -> Result<String, OrchestratorError> {
        self.store.create_tag_object(
            repository,
            &JournalTag {
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
        self.store.create_ref(repository, name, target)
    }
    fn get_tag(
        &self,
        repository: &str,
        sha: &str,
    ) -> Result<crate::crate_release_journal_adapter::RemoteTag, OrchestratorError> {
        Ok(crate::crate_release_journal_adapter::RemoteTag {
            sha: sha.to_owned(),
            tag: self.store.get_tag_object(repository, sha)?,
            tagger_name: "Velnor Release".into(),
            tagger_email: "release@velnor.invalid".into(),
            tagger_date: "1970-01-01T00:00:00Z".into(),
            unsigned: self.unsigned,
        })
    }
    fn get_blob(&self, repository: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        self.store.get_blob(repository, sha)
    }
}

#[test]
fn canonical_unsigned_v1_v2_records_require_github_custody()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal_adapter::JournalAdapter;
    for schema in [1, 2] {
        let mut plan = receipt("0.1.1");
        if schema == 2 {
            plan.packages[0].tag_target = Some("f".repeat(40));
        }
        let store = Registry::default();
        seed_started_record(&store, &identity(), &plan)?;
        let adapter = JournalAdapter::new(CustodyTransport {
            store: &store,
            unsigned: true,
            deny_rules: false,
        });
        assert_eq!(
            inspect_journal(&adapter, &identity(), JournalStage::Started)?
                .ok_or("missing")?
                .receipt,
            plan
        );
        let denied = JournalAdapter::new(CustodyTransport {
            store: &store,
            unsigned: true,
            deny_rules: true,
        });
        assert!(inspect_journal(&denied, &identity(), JournalStage::Started).is_err());
        let wrong_custody = JournalAdapter::new(CustodyTransport {
            store: &store,
            unsigned: false,
            deny_rules: false,
        });
        assert!(inspect_journal(&wrong_custody, &identity(), JournalStage::Started).is_err());
        store
            .blobs
            .borrow_mut()
            .values_mut()
            .next()
            .ok_or("blob")?
            .push(b' ');
        let tampered = JournalAdapter::new(CustodyTransport {
            store: &store,
            unsigned: true,
            deny_rules: false,
        });
        assert!(inspect_journal(&tampered, &identity(), JournalStage::Started).is_err());
    }
    Ok(())
}

impl crate::crate_release_journal_adapter::JournalRefTransport for CustodyTransport<'_> {
    fn list_operations(
        &self,
        identity: &JournalIdentity,
    ) -> Result<Vec<(String, String)>, OrchestratorError> {
        self.store.list_operation_refs(identity)
    }
}

#[test]
fn canonical_adapter_runs_v1_and_v2_start_mutation_completion()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalMutation, JournalMutationKind, JournalMutationPhase, JournalMutationResult,
        inspect_journal_mutation, record_journal_mutation,
    };
    use crate::crate_release_journal_adapter::JournalAdapter;
    for schema in [1, 2] {
        let store = Registry::default();
        let adapter = JournalAdapter::new(CustodyTransport {
            store: &store,
            unsigned: true,
            deny_rules: false,
        });
        let mut plan = receipt("0.1.1");
        if schema == 2 {
            plan.packages[0].tag_target = Some("f".repeat(40));
        }
        record_journal(&adapter, &identity(), JournalStage::Started, &plan, None)?;
        let mut mutation = JournalMutation {
            archive_sha256: None,
            semver_evidence: Some(
                crate::release::crate_release_semver::SemverEvidence::NoBaseline {
                    checker_sha256: velnor_actions_mise::catalog::qualified_cargo_semver_checks(
                        "ubuntu-26.04",
                    )
                    .ok_or("checker")?
                    .executable_sha256
                    .into(),
                },
            ),
            sequence: 1,
            package: "demo-crate".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
            phase: JournalMutationPhase::Intended,
        };
        record_journal_mutation(&adapter, &identity(), &plan, &mutation)?;
        mutation.sequence = 2;
        mutation.semver_evidence = None;
        mutation.phase = JournalMutationPhase::Observed {
            result: JournalMutationResult::Upload {
                registry_checksum: "d".repeat(64),
                downloaded_checksum: "d".repeat(64),
                owners: journal_existing_owners(&plan.packages[0].owners),
                features: BTreeMap::new(),
                files: completion("0.1.1").packages[0].files.clone(),
            },
        };
        record_journal_mutation(&adapter, &identity(), &plan, &mutation)?;
        assert_eq!(
            inspect_journal_mutation(&adapter, &identity(), 2)?,
            Some(mutation)
        );
        let mut done = completion("0.1.1");
        if schema == 2 {
            done.packages[0].tag_target = "f".repeat(40);
        }
        record_journal(
            &adapter,
            &identity(),
            JournalStage::Completed,
            &plan,
            Some(&done),
        )?;
        assert_eq!(
            inspect_journal(&adapter, &identity(), JournalStage::Completed)?
                .ok_or("missing")?
                .completion,
            Some(done)
        );
    }
    Ok(())
}

#[test]
fn reads_exact_pre_schema_v1_bytes_with_custody() -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal_adapter::JournalAdapter;
    let bytes = include_bytes!("../fixtures/historical-journal/v1-start-07f76be.json");
    assert_eq!(
        crate::planner::generator::sha256_hex(bytes),
        "af16f9a360762473ca516febcc84212db8b7714e5eb5394ccedb2124b1778dc6"
    );
    let store = Registry::default();
    let repo = &identity().repository;
    let blob = store.create_blob(repo, bytes)?;
    let name = "velnor-receipt/42/1/publication-started";
    let tag = JournalTag {
        name: name.into(),
        object_type: "blob".into(),
        object_sha: blob,
        message: "velnor-release-receipt-v1".into(),
    };
    let sha = store.create_tag_object(repo, &tag)?;
    store.create_ref(repo, &format!("refs/tags/{name}"), &sha)?;
    let reader = JournalAdapter::new(CustodyTransport {
        store: &store,
        unsigned: true,
        deny_rules: false,
    });
    let actual = inspect_journal(&reader, &identity(), JournalStage::Started)?.ok_or("missing")?;
    assert_eq!(actual.receipt, receipt("0.1.1"));
    let denied = JournalAdapter::new(CustodyTransport {
        store: &store,
        unsigned: true,
        deny_rules: true,
    });
    assert!(inspect_journal(&denied, &identity(), JournalStage::Started).is_err());
    let wrong = JournalAdapter::new(CustodyTransport {
        store: &store,
        unsigned: false,
        deny_rules: false,
    });
    assert!(inspect_journal(&wrong, &identity(), JournalStage::Started).is_err());
    store
        .blobs
        .borrow_mut()
        .values_mut()
        .next()
        .ok_or("blob")?
        .push(b' ');
    let tampered = JournalAdapter::new(CustodyTransport {
        store: &store,
        unsigned: true,
        deny_rules: false,
    });
    assert!(inspect_journal(&tampered, &identity(), JournalStage::Started).is_err());
    Ok(())
}
