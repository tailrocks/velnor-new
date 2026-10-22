use super::*;

#[test]
fn selected_metadata_rejects_expiry_size_digest_source_fork_and_old_attempt_name() {
    let mutations: Vec<fn(&mut ArtifactRecord)> = vec![
        |a| a.expired = true,
        |a| a.size_in_bytes = 0,
        |a| a.size_in_bytes = ZipProfile::PreviewLeaf.compressed() + 1,
        |a| a.digest = None,
        |a| a.digest = Some("sha256:not-a-digest".to_owned()),
        |a| a.name = a.name.replace("-a2-", "-a1-"),
        |a| a.workflow_run = None,
        |a| a.workflow_run.as_mut().expect("run").id += 1,
        |a| a.workflow_run.as_mut().expect("run").repository_id += 1,
        |a| a.workflow_run.as_mut().expect("run").head_repository_id += 1,
        |a| a.workflow_run.as_mut().expect("run").head_sha = "c".repeat(40),
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
fn selected_metadata_preserves_the_service_assigned_artifact_id() {
    let mut artifact = record();
    artifact.id = 909;
    let api = Fake::new(vec![artifact]);
    let admitted = acquire_fixture(&api, &request(), deadline()).expect("service ID accepted");
    assert_eq!(admitted[0].reference.artifact_id, 909);
    assert_eq!(*api.downloads.borrow(), [909]);
}

#[test]
fn well_formed_service_digest_is_checked_against_downloaded_archive_bytes() {
    let mut artifact = record();
    artifact.digest = Some(format!("sha256:{}", "b".repeat(64)));
    let api = Fake::new(vec![artifact]);
    assert!(acquire_fixture(&api, &request(), deadline()).is_err());
    assert_eq!(*api.downloads.borrow(), [303]);
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
    invalid.context.workflow_authority_sha = "not-a-sha".to_owned();
    assert!(acquire_fixture(&api, &invalid, deadline()).is_err());
    assert_eq!(api.authentications.get(), 0);
    assert!(api.downloads.borrow().is_empty());

    let mut supplied = request();
    let ReleaseProductionStage::Assemble { inputs } = &mut supplied.stage else {
        unreachable!()
    };
    inputs.push(PayloadArtifactRef {
        pair: velnor_actions_contract::preview_production::ReleasePayloadPair {
            product_id: "app".to_owned(),
            target: "aarch64-apple-darwin".to_owned(),
        },
        artifact_id: 303,
        sha256: sha256(b"closed ZIP bytes"),
    });
    assert!(acquire_fixture(&api, &supplied, deadline()).is_err());
    assert_eq!(api.authentications.get(), 0);
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
    request.stage = ReleaseProductionStage::Assemble { inputs: Vec::new() };
    let api = Fake::new(records);
    configure_jobs(&api, &request);
    let error = acquire_fixture(&api, &request, deadline()).expect_err("complete union bound");
    assert!(
        error.to_string().contains("preview_artifact_total_size"),
        "{error}"
    );
    assert!(api.downloads.borrow().is_empty());
}
