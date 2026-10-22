use super::super::github::sha256;
use super::discovery::discover_preview_inputs;
use super::fixture::{record, request};
use super::*;
use std::io::{Read, Write};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    time::Duration,
};

struct Fake {
    pages: RefCell<VecDeque<ArtifactPage>>,
    job_names: RefCell<Vec<String>>,
    job_sha: RefCell<String>,
    authentications: Cell<usize>,
    fail_authentication: Option<usize>,
    bytes: Vec<u8>,
    downloads: RefCell<Vec<u64>>,
}
impl Fake {
    fn new(records: Vec<ArtifactRecord>) -> Self {
        Self {
            pages: RefCell::new(VecDeque::from([ArtifactPage {
                total_count: records.len(),
                artifacts: records,
            }])),
            job_names: RefCell::new(vec!["Build app / aarch64-apple-darwin".to_owned()]),
            job_sha: RefCell::new("b".repeat(40)),
            authentications: Cell::new(0),
            fail_authentication: None,
            bytes: b"closed ZIP bytes".to_vec(),
            downloads: RefCell::new(vec![]),
        }
    }
}
impl ArtifactReadApi for Fake {
    fn authenticate(
        &self,
        _: &PackageReleasePlan,
        _: &ReleaseProductionContext,
        consumer: ReleaseConsumer,
    ) -> Result<(), OrchestratorError> {
        assert_eq!(consumer, ReleaseConsumer::Assembler);
        let call = self.authentications.get() + 1;
        self.authentications.set(call);
        if self.fail_authentication == Some(call) {
            return Err(internal("fixture_stale_attempt"));
        }
        Ok(())
    }
    fn page(&self, _: u64, _: usize) -> Result<ArtifactPage, OrchestratorError> {
        self.pages
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| internal("unexpected_page"))
    }
    fn jobs(&self, run: u64, _: u32) -> Result<Vec<JobRecord>, OrchestratorError> {
        Ok(self
            .job_names
            .borrow()
            .iter()
            .map(|name| JobRecord {
                run_id: run,
                name: name.clone(),
                head_sha: Some(self.job_sha.borrow().clone()),
                status: Some("completed".to_owned()),
                conclusion: Some("success".to_owned()),
            })
            .collect())
    }
    fn download(
        &self,
        id: u64,
        size: u64,
        output: &mut File,
        _: CheckDeadline,
    ) -> Result<(), OrchestratorError> {
        assert_eq!(size, 16, "download limit is the authenticated service size");
        self.downloads.borrow_mut().push(id);
        output
            .write_all(&self.bytes)
            .map_err(|_| internal("fixture_write"))
    }
}
fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(2)).expect("deadline")
}

#[derive(Debug)]
struct TestArtifact {
    reference: PayloadArtifactRef,
    bytes: Vec<u8>,
}
fn acquire_fixture(
    api: &impl ArtifactReadApi,
    request: &ReleaseProductionRequest,
    deadline: CheckDeadline,
) -> Result<Vec<TestArtifact>, OrchestratorError> {
    let root = tempfile::tempdir().expect("download directory");
    let mut rows = Vec::new();
    super::acquire(
        api,
        request,
        &root.path().canonicalize().expect("canonical"),
        deadline,
        &mut |mut artifact| {
            let mut bytes = Vec::new();
            artifact
                .archive
                .reader()?
                .read_to_end(&mut bytes)
                .map_err(|_| internal("fixture_read"))?;
            rows.push(TestArtifact {
                reference: artifact.reference,
                bytes,
            });
            Ok(())
        },
    )?;
    assert_eq!(
        std::fs::read_dir(root.path())
            .expect("download directory")
            .count(),
        0,
        "each archive released before the next leaf"
    );
    Ok(rows)
}

fn configure_jobs(api: &Fake, request: &ReleaseProductionRequest) {
    *api.job_names.borrow_mut() =
        velnor_actions_contract::preview_production::expected_release_payload_pairs(&request.plan)
            .expect("pairs")
            .into_iter()
            .map(|pair| format!("Build {} / {}", pair.product_id, pair.target))
            .collect();
    *api.job_sha.borrow_mut() = request.context.workflow_authority_sha.clone();
}

/// Exercise the same fake authenticated transport used by inventory tests from
/// the composed source-acquisition test.
pub(crate) fn discover_request_for_sealing_test(
    request: &ReleaseProductionRequest,
) -> Result<ReleaseProductionRequest, OrchestratorError> {
    let pairs =
        velnor_actions_contract::preview_production::expected_release_payload_pairs(&request.plan)
            .map_err(|_| internal("fixture_pairs"))?;
    let artifacts = pairs
        .iter()
        .enumerate()
        .map(|(index, pair)| {
            let mut artifact = record();
            artifact.id = u64::try_from(index + 303).map_err(|_| internal("fixture_id"))?;
            artifact.name = release_payload_artifact_name(&request.plan, pair, &request.context)
                .map_err(|_| internal("fixture_name"))?;
            let source = artifact
                .workflow_run
                .as_mut()
                .ok_or_else(|| internal("fixture_source"))?;
            source.id = request.context.run_id;
            source.repository_id = request.context.repository_id;
            source.head_repository_id = request.context.head_repository_id;
            source.head_sha = request.context.workflow_authority_sha.clone();
            Ok(artifact)
        })
        .collect::<Result<Vec<_>, OrchestratorError>>()?;
    let api = Fake::new(artifacts);
    configure_jobs(&api, request);
    let root = tempfile::tempdir().expect("download directory");
    let directory = root.path().canonicalize().expect("canonical directory");
    super::acquire(&api, request, &directory, deadline(), &mut |_| Ok(()))
}

#[test]
fn exact_id_digest_size_and_current_attempt_admit_only_service_archive_bytes() {
    let api = Fake::new(vec![record()]);
    let admitted = acquire_fixture(&api, &request(), deadline()).expect("exact identity");
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].bytes, b"closed ZIP bytes");
    assert_eq!(admitted[0].reference.artifact_id, 303);
    assert_eq!(admitted[0].bytes.len(), 16);
    assert_eq!(sha256(&admitted[0].bytes), admitted[0].reference.sha256);
    assert_eq!(*api.downloads.borrow(), [303]);
    assert_eq!(api.authentications.get(), 2);
}

#[test]
fn preview_assembly_discovers_complete_union_without_needs_inputs() {
    let mut request = request();
    let ReleaseProductionStage::Assemble { inputs } = &mut request.stage else {
        unreachable!()
    };
    inputs.clear();
    let api = Fake::new(vec![record()]);
    let admitted = acquire_fixture(&api, &request, deadline()).expect("API-discovered union");
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].reference.artifact_id, 303);
    assert_eq!(admitted[0].reference.pair.product_id, "app");
}

#[test]
fn preview_acquisition_returns_the_exact_discovered_union_for_sealing() {
    let request = request();
    let api = Fake::new(vec![record()]);
    let root = tempfile::tempdir().expect("download directory");
    let directory = root.path().canonicalize().expect("canonical directory");
    let completed = super::acquire(&api, &request, &directory, deadline(), &mut |_| Ok(()))
        .expect("API discovers and returns complete request");
    let ReleaseProductionStage::Assemble { inputs } = &completed.stage else {
        panic!("complete assembly")
    };
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].artifact_id, 303);
    assert_eq!(inputs[0].sha256, sha256(b"closed ZIP bytes"));
    assert_eq!(completed.context.source_sha, request.context.source_sha);
    assert_eq!(
        completed.context.workflow_authority_sha,
        request.context.workflow_authority_sha
    );
}

#[test]
fn preview_union_rejects_incomplete_foreign_duplicate_and_unexpected_entries() {
    let request = request();
    let expected_job = JobRecord {
        run_id: request.context.run_id,
        name: "Build app / aarch64-apple-darwin".into(),
        head_sha: Some(request.context.workflow_authority_sha.clone()),
        status: Some("completed".into()),
        conclusion: Some("success".into()),
    };
    let artifact = record();
    let valid = BTreeMap::from([(artifact.id, artifact.clone())]);
    assert_eq!(
        discover_preview_inputs(&request, &valid, std::slice::from_ref(&expected_job))
            .expect("complete source-bound union")[0]
            .artifact_id,
        artifact.id
    );
    assert!(
        discover_preview_inputs(
            &request,
            &BTreeMap::new(),
            std::slice::from_ref(&expected_job)
        )
        .is_err()
    );
    let mut duplicate = artifact.clone();
    duplicate.id += 1;
    let duplicate_inventory =
        BTreeMap::from([(artifact.id, artifact.clone()), (duplicate.id, duplicate)]);
    assert!(
        discover_preview_inputs(
            &request,
            &duplicate_inventory,
            std::slice::from_ref(&expected_job)
        )
        .is_err()
    );
    let mut extra = artifact;
    extra.id += 1;
    extra.name = format!(
        "velnor-release-payload-r{}-a{}-unexpected",
        request.context.run_id, request.context.run_attempt
    );
    assert!(
        discover_preview_inputs(
            &request,
            &BTreeMap::from([(303, record()), (extra.id, extra)]),
            std::slice::from_ref(&expected_job)
        )
        .is_err()
    );
    let mut foreign = expected_job.clone();
    foreign.run_id += 1;
    assert!(discover_preview_inputs(&request, &valid, &[foreign]).is_err());
    let mut wrong_authority = expected_job.clone();
    wrong_authority.head_sha = Some(request.context.source_sha.clone());
    assert!(discover_preview_inputs(&request, &valid, &[wrong_authority]).is_err());
    assert!(discover_preview_inputs(&request, &valid, &[]).is_err());
}

#[test]
fn independently_valid_leaves_exceed_the_old_union_cap_and_are_consumed_one_at_a_time() {
    let mut request = request();
    request.plan.products[0]
        .targets
        .push("x86_64-apple-darwin".into());
    request.plan_sha256 =
        velnor_actions_contract::preview_production::release_plan_sha256(&request.plan)
            .expect("full plan");
    let references =
        velnor_actions_contract::preview_production::expected_release_payload_pairs(&request.plan)
            .expect("complete plan")
            .into_iter()
            .enumerate()
            .map(|(index, pair)| PayloadArtifactRef {
                pair,
                artifact_id: 303 + u64::try_from(index).expect("small fixture index"),
                sha256: sha256(b"closed ZIP bytes"),
            })
            .collect::<Vec<_>>();
    let records = references
        .iter()
        .map(|reference| ArtifactRecord {
            id: reference.artifact_id,
            name: release_payload_artifact_name(&request.plan, &reference.pair, &request.context)
                .expect("name"),
            ..record()
        })
        .collect::<Vec<_>>();
    for (artifact, reference) in records.iter().zip(references.iter()) {
        let large = ArtifactRecord {
            size_in_bytes: 600 * 1024 * 1024,
            ..artifact.clone()
        };
        validate_selected(&large, reference, &request)
            .expect("valid leaf above half the old union cap");
    }
    let observed_total: u64 = records.iter().map(|_| 600 * 1024 * 1024).sum();
    assert!(observed_total > ZipProfile::Recovery.compressed());
    assert!(observed_total < leaf_union_compressed_bound(records.len()));
    let api = Fake::new(records);
    configure_jobs(&api, &request);
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().canonicalize().expect("canonical");
    let mut seen = Vec::new();
    let completed = super::acquire(
        &api,
        &request,
        &directory,
        deadline(),
        &mut |mut artifact| {
            assert_eq!(
                std::fs::read_dir(&directory)
                    .expect("live archives")
                    .count(),
                1
            );
            artifact.archive.verify(deadline())?;
            seen.push(artifact.reference.artifact_id);
            Ok(())
        },
    )
    .expect("every leaf consumed");
    let ReleaseProductionStage::Assemble { inputs } = completed.stage else {
        panic!("completed API-discovered assembly")
    };
    assert_eq!(inputs, references);
    assert_eq!(seen, [303, 304]);
    assert_eq!(*api.downloads.borrow(), [303, 304]);
    assert_eq!(api.authentications.get(), 2);
    assert_eq!(
        std::fs::read_dir(&directory)
            .expect("released archives")
            .count(),
        0
    );
}

#[test]
fn inventory_requires_stable_totals_full_pages_unique_positive_ids_and_count_bound() {
    let full = (1..=100)
        .map(|id| ArtifactRecord { id, ..record() })
        .collect::<Vec<_>>();
    let valid = vec![
        ArtifactPage {
            total_count: 101,
            artifacts: full.clone(),
        },
        ArtifactPage {
            total_count: 101,
            artifacts: vec![ArtifactRecord {
                id: 101,
                ..record()
            }],
        },
    ];
    let api = Fake::new(vec![]);
    *api.pages.borrow_mut() = valid.clone().into();
    assert_eq!(
        inventory(&api, 202, deadline()).expect("all pages").len(),
        101
    );
    let mut cases = vec![
        vec![ArtifactPage {
            total_count: 2,
            artifacts: vec![record()],
        }],
        vec![ArtifactPage {
            total_count: 1,
            artifacts: vec![],
        }],
        vec![ArtifactPage {
            total_count: 0,
            artifacts: vec![record()],
        }],
        vec![ArtifactPage {
            total_count: 1001,
            artifacts: full,
        }],
        vec![ArtifactPage {
            total_count: 2,
            artifacts: vec![record(), record()],
        }],
        vec![ArtifactPage {
            total_count: 1,
            artifacts: vec![ArtifactRecord { id: 0, ..record() }],
        }],
    ];
    let mut changed = valid.clone();
    changed[1].total_count = 102;
    cases.push(changed);
    let mut short = valid.clone();
    short[1].artifacts.clear();
    cases.push(short);
    let mut duplicate = valid;
    duplicate[1].artifacts[0].id = 1;
    cases.push(duplicate);
    for pages in cases {
        let api = Fake::new(vec![]);
        *api.pages.borrow_mut() = pages.into();
        assert!(inventory(&api, 202, deadline()).is_err());
        assert!(api.downloads.borrow().is_empty());
    }
}

#[path = "preview_artifact_transport_boundary_tests.rs"]
mod boundary_tests;
