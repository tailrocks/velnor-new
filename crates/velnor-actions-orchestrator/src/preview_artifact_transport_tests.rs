use super::fixture::{record, request};
use super::*;
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
        _: &PreviewProductionContext,
        consumer: PreviewConsumer,
    ) -> Result<(), OrchestratorError> {
        assert_eq!(consumer, PreviewConsumer::Assembler);
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
    fn download(&self, id: u64, size: u64) -> Result<Vec<u8>, OrchestratorError> {
        assert_eq!(size, 16, "download limit is the authenticated service size");
        self.downloads.borrow_mut().push(id);
        Ok(self.bytes.clone())
    }
}
fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(2)).expect("deadline")
}

#[test]
fn exact_id_digest_size_and_current_attempt_admit_only_service_archive_bytes() {
    let api = Fake::new(vec![record()]);
    let admitted = acquire(&api, &request(), deadline()).expect("exact identity");
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].bytes, b"closed ZIP bytes");
    assert_eq!(admitted[0].reference.artifact_id, 303);
    assert_eq!(admitted[0].bytes.len(), 16);
    assert_eq!(sha256(&admitted[0].bytes), admitted[0].reference.sha256);
    assert_eq!(*api.downloads.borrow(), [303]);
    assert_eq!(api.authentications.get(), 2);
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
        |a| a.size_in_bytes = MAX_ARCHIVE_BYTES + 1,
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
        assert!(acquire(&api, &request(), deadline()).is_err());
        assert!(api.downloads.borrow().is_empty());
    }
}

#[test]
fn same_size_content_conflict_size_conflict_or_final_attempt_change_return_no_bytes() {
    for bytes in [b"different bytes!".to_vec(), b"short".to_vec()] {
        let mut api = Fake::new(vec![record()]);
        api.bytes = bytes;
        assert!(acquire(&api, &request(), deadline()).is_err());
        assert_eq!(api.authentications.get(), 1);
    }
    for call in [1, 2] {
        let mut api = Fake::new(vec![record()]);
        api.fail_authentication = Some(call);
        assert!(acquire(&api, &request(), deadline()).is_err());
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
    assert!(acquire(&api, &request(), expired).is_err());
    let mut invalid = request();
    invalid.context.workflow_authority_sha = "b".repeat(40);
    assert!(acquire(&api, &invalid, deadline()).is_err());
    assert_eq!(api.authentications.get(), 0);
    assert!(api.downloads.borrow().is_empty());
}

#[test]
fn complete_metadata_union_exceeding_total_byte_bound_downloads_nothing() {
    let mut request = request();
    request.plan.products[0]
        .targets
        .push("x86_64-apple-darwin".into());
    request.plan_sha256 =
        velnor_actions_contract::preview_production::preview_plan_sha256(&request.plan)
            .expect("expanded plan digest");
    let PreviewProductionStage::Assemble { inputs } = &mut request.stage else {
        unreachable!()
    };
    let mut second = inputs[0].clone();
    second.artifact_id = 304;
    second.pair.target = "x86_64-apple-darwin".into();
    inputs.push(second);
    let PreviewProductionStage::Assemble { inputs } = &request.stage else {
        unreachable!()
    };
    let mut first = record();
    first.size_in_bytes = MAX_TOTAL_BYTES / 2 + 1;
    let mut second = first.clone();
    second.id = 304;
    second.name = preview_payload_artifact_name(&request.plan, &inputs[1].pair, &request.context)
        .expect("second canonical identity");
    request.validate().expect("complete two-pair request");
    let api = Fake::new(vec![first, second]);
    assert!(acquire(&api, &request, deadline()).is_err());
    assert!(api.downloads.borrow().is_empty());
}

#[test]
fn native_consumer_environment_is_closed_and_role_specific() {
    for role in [PreviewConsumer::Assembler, PreviewConsumer::Signer] {
        validate_consumer_environment(role, Some(role.job_name())).expect("exact native role");
        for actual in [
            None,
            Some("preview-producer"),
            Some("preview-payload-000"),
            Some("unknown"),
        ] {
            assert!(validate_consumer_environment(role, actual).is_err());
        }
    }
    assert!(
        validate_consumer_environment(PreviewConsumer::Signer, Some(PREVIEW_ASSEMBLE_JOB_ID))
            .is_err()
    );
    assert!(
        validate_consumer_environment(PreviewConsumer::Assembler, Some("preview-signer")).is_err()
    );
}
