use super::super::github::sha256;
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
        Ok(vec![JobRecord {
            run_id: run,
            name: "Build app / aarch64-apple-darwin".to_owned(),
            head_sha: Some("b".repeat(40)),
            status: Some("completed".to_owned()),
            conclusion: Some("success".to_owned()),
        }])
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
    let ReleaseProductionStage::Assemble { inputs } = &mut request.stage else {
        unreachable!()
    };
    let mut second = inputs[0].clone();
    second.artifact_id = 304;
    second.pair.target = "x86_64-apple-darwin".into();
    inputs.push(second);
    let ReleaseProductionStage::Assemble { inputs } = &request.stage else {
        unreachable!()
    };
    let records = inputs
        .iter()
        .map(|reference| ArtifactRecord {
            id: reference.artifact_id,
            name: release_payload_artifact_name(&request.plan, &reference.pair, &request.context)
                .expect("name"),
            ..record()
        })
        .collect::<Vec<_>>();
    for (artifact, reference) in records.iter().zip(inputs.iter()) {
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
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().canonicalize().expect("canonical");
    let mut seen = Vec::new();
    super::acquire(
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

#[test]
fn selected_metadata_rejects_expiry_size_digest_source_fork_and_old_attempt_name() {
    let mutations: Vec<fn(&mut ArtifactRecord)> = vec![
        |a| a.expired = true,
        |a| a.size_in_bytes = 0,
        |a| a.size_in_bytes = ZipProfile::PreviewLeaf.compressed() + 1,
        |a| a.digest = None,
        |a| a.digest = Some(format!("sha256:{}", "b".repeat(64))),
        |a| a.name = a.name.replace("-a2-", "-a1-"),
        |a| a.id += 1,
        |a| a.workflow_run = None,
        |a| a.workflow_run.as_mut().expect("run").id += 1,
        |a| a.workflow_run.as_mut().expect("run").repository_id += 1,
        |a| a.workflow_run.as_mut().expect("run").head_repository_id += 1,
        |a| a.workflow_run.as_mut().expect("run").head_sha = "b".repeat(40),
        |a| a.workflow_run.as_mut().expect("run").head_branch = "other".into(),
    ];
    for mutate in mutations {
        let mut artifact = record();
        mutate(&mut artifact);
        let api = Fake::new(vec![artifact]);
        assert!(acquire_fixture(&api, &request(), deadline()).is_err());
        assert!(api.downloads.borrow().is_empty());
    }
}

#[test]
fn same_size_content_conflict_size_conflict_or_final_attempt_change_return_no_bytes() {
    for bytes in [b"different bytes!".to_vec(), b"short".to_vec()] {
        let mut api = Fake::new(vec![record()]);
        api.bytes = bytes;
        assert!(acquire_fixture(&api, &request(), deadline()).is_err());
        assert_eq!(api.authentications.get(), 1);
    }
    for call in [1, 2] {
        let mut api = Fake::new(vec![record()]);
        api.fail_authentication = Some(call);
        assert!(acquire_fixture(&api, &request(), deadline()).is_err());
        assert_eq!(api.downloads.borrow().len(), usize::from(call == 2));
    }
}

#[test]
fn exhausted_deadline_and_invalid_requests_do_not_start_transport() {
    let api = Fake::new(vec![record()]);
    let expired = CheckDeadline::from_start(
        std::time::Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("one second before test clock"),
        Duration::from_millis(1),
    )
    .expect("past deadline");
    assert!(acquire_fixture(&api, &request(), expired).is_err());
    let mut invalid = request();
    invalid.context.workflow_authority_sha = "b".repeat(40);
    assert!(acquire_fixture(&api, &invalid, deadline()).is_err());
    assert_eq!(api.authentications.get(), 0);
    assert!(api.downloads.borrow().is_empty());
}

#[test]
fn complete_metadata_union_exceeding_total_byte_bound_downloads_nothing() {
    let mut request = request();
    let template = request.plan.products[0].clone();
    request.plan.products = (0..8)
        .map(|index| {
            let mut product = template.clone();
            product.id = format!("app-{index:03}");
            product.package = product.id.clone();
            product.bins = vec![product.id.clone()];
            product
        })
        .collect();
    request.plan_sha256 =
        velnor_actions_contract::preview_production::release_plan_sha256(&request.plan)
            .expect("full plan");
    let pairs =
        velnor_actions_contract::preview_production::expected_release_payload_pairs(&request.plan)
            .expect("eight pairs");
    let inputs = pairs
        .into_iter()
        .enumerate()
        .map(|(index, pair)| PayloadArtifactRef {
            pair,
            artifact_id: 303 + index as u64,
            sha256: sha256(b"closed ZIP bytes"),
        })
        .collect::<Vec<_>>();
    let records = inputs
        .iter()
        .map(|reference| ArtifactRecord {
            id: reference.artifact_id,
            name: release_payload_artifact_name(&request.plan, &reference.pair, &request.context)
                .expect("name"),
            size_in_bytes: 1024 * 1024 * 1024,
            ..record()
        })
        .collect::<Vec<_>>();
    request.stage = ReleaseProductionStage::Assemble { inputs };
    request.validate().expect("complete eight-pair request");
    let ReleaseProductionStage::Assemble { inputs } = &request.stage else {
        unreachable!()
    };
    for (artifact, reference) in records.iter().zip(inputs) {
        validate_selected(artifact, reference, &request)
            .expect("each individual leaf is within its own bound");
    }
    let api = Fake::new(records);
    let error = acquire_fixture(&api, &request, deadline()).expect_err("complete union bound");
    assert!(
        error.to_string().contains("preview_artifact_total_size"),
        "{error}"
    );
    assert!(api.downloads.borrow().is_empty());
}
