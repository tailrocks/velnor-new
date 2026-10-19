use super::*;

#[path = "preview_production_test_support.rs"]
mod support;
use support::{assembly, request};

#[test]
fn six_pairs_preserve_complete_product_bins_and_full_plan() {
    let request = request();
    let pairs = expected_preview_payload_pairs(&request.plan).expect("six pairs");
    let actual = pairs
        .iter()
        .map(|pair| (pair.product_id.as_str(), pair.target.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            ("jackin", "aarch64-apple-darwin"),
            ("jackin", "aarch64-unknown-linux-gnu"),
            ("jackin", "x86_64-apple-darwin"),
            ("jackin", "x86_64-unknown-linux-gnu"),
            ("jackin-capsule", "aarch64-unknown-linux-gnu"),
            ("jackin-capsule", "x86_64-unknown-linux-gnu"),
        ]
    );
    assert_eq!(request.plan.products[0].bins.len(), 3);
    assert_eq!(PREVIEW_ASSEMBLE_JOB_ID, "preview-assemble");
    for (index, pair) in pairs.into_iter().enumerate() {
        assert_eq!(
            preview_payload_job_id(&request.plan, &pair).expect("fixed job"),
            format!("preview-payload-{index:03}")
        );
        let mut leaf = request.clone();
        leaf.stage = PreviewProductionStage::Payload { pair };
        let bytes = leaf.canonical_bytes().expect("leaf bytes");
        assert_eq!(
            parse_preview_production_request(&bytes).expect("leaf"),
            leaf
        );
        leaf.validate_against(&request.plan, &request.context)
            .expect("full authority");
        assert_eq!(leaf.plan, request.plan);
    }
    let union = assembly();
    let bytes = union.canonical_bytes().expect("union bytes");
    assert_eq!(
        parse_preview_production_request(&bytes).expect("union"),
        union
    );
}

#[test]
fn assembly_rejects_missing_extra_duplicate_unsorted_and_foreign_pairs() {
    for mutate in [
        |inputs: &mut Vec<PayloadArtifactRef>| {
            inputs.pop();
        },
        |inputs: &mut Vec<PayloadArtifactRef>| inputs.push(inputs[0].clone()),
        |inputs: &mut Vec<PayloadArtifactRef>| inputs[1] = inputs[0].clone(),
        |inputs: &mut Vec<PayloadArtifactRef>| inputs.swap(0, 1),
        |inputs: &mut Vec<PayloadArtifactRef>| inputs[0].pair.product_id = "foreign".to_owned(),
        |inputs: &mut Vec<PayloadArtifactRef>| inputs[0].pair.target = "unknown".to_owned(),
        Vec::clear,
    ] {
        let mut candidate = assembly();
        let PreviewProductionStage::Assemble { inputs } = &mut candidate.stage else {
            unreachable!()
        };
        mutate(inputs);
        assert!(candidate.validate().is_err());
    }
}

#[test]
fn artifact_refs_require_distinct_positive_ids_and_exact_lowercase_sha256() {
    for mutate in [
        |inputs: &mut Vec<PayloadArtifactRef>| inputs[0].artifact_id = 0,
        |inputs: &mut Vec<PayloadArtifactRef>| inputs[1].artifact_id = inputs[0].artifact_id,
        |inputs: &mut Vec<PayloadArtifactRef>| inputs[0].sha256 = "B".repeat(64),
        |inputs: &mut Vec<PayloadArtifactRef>| inputs[0].sha256 = "b".repeat(63),
        |inputs: &mut Vec<PayloadArtifactRef>| inputs[0].sha256 = "g".repeat(64),
        |inputs: &mut Vec<PayloadArtifactRef>| {
            inputs[0].sha256 = format!("sha256:{}", "b".repeat(64));
        },
        |inputs: &mut Vec<PayloadArtifactRef>| inputs[0].sha256.clear(),
    ] {
        let mut candidate = assembly();
        let PreviewProductionStage::Assemble { inputs } = &mut candidate.stage else {
            unreachable!()
        };
        mutate(inputs);
        assert!(candidate.validate().is_err());
    }
}

#[test]
fn context_rejects_forks_refs_authority_versions_and_nonpositive_identities() {
    for mutate in [
        |context: &mut PreviewProductionContext| context.repository_id = 0,
        |context: &mut PreviewProductionContext| context.head_repository_id += 1,
        |context: &mut PreviewProductionContext| context.source_sha = "A".repeat(40),
        |context: &mut PreviewProductionContext| context.source_sha = "a".repeat(7),
        |context: &mut PreviewProductionContext| {
            context.source_ref = "refs/pull/1/merge".to_owned();
        },
        |context: &mut PreviewProductionContext| context.workflow_authority_sha = "b".repeat(40),
        |context: &mut PreviewProductionContext| context.run_id = 0,
        |context: &mut PreviewProductionContext| context.run_attempt = 0,
        |context: &mut PreviewProductionContext| context.ci_run_id = 0,
        |context: &mut PreviewProductionContext| context.ci_run_attempt = 0,
        |context: &mut PreviewProductionContext| context.commit_count = 0,
        |context: &mut PreviewProductionContext| context.version.push_str("-forged"),
    ] {
        let mut candidate = request();
        mutate(&mut candidate.context);
        assert!(candidate.validate().is_err(), "{:?}", candidate.context);
        assert!(validate_preview_production_context(&candidate.plan, &candidate.context).is_err());
        assert!(preview_aggregate_artifact_name(&candidate.plan, &candidate.context).is_err());
    }
}

#[test]
fn trusted_comparison_rejects_self_consistent_partial_plan_and_stale_attempt() {
    let trusted = request();
    let mut partial = trusted.clone();
    partial.plan.products.pop();
    partial.plan_sha256 = preview_plan_sha256(&partial.plan).expect("smaller valid plan");
    partial
        .validate()
        .expect("syntax cannot know remote full policy");
    assert!(
        partial
            .validate_against(&trusted.plan, &trusted.context)
            .is_err()
    );
    let mut stale = trusted.clone();
    stale.context.run_attempt = 1;
    stale
        .validate()
        .expect("syntax cannot authenticate current attempt");
    assert!(
        stale
            .validate_against(&trusted.plan, &trusted.context)
            .is_err()
    );
    let mut stale_ci = trusted.clone();
    stale_ci.context.ci_run_attempt += 1;
    assert!(
        stale_ci
            .validate_against(&trusted.plan, &trusted.context)
            .is_err()
    );
}

#[test]
fn full_plan_hash_covers_all_products_and_companions_without_changing_plan() {
    let original = request();
    // Independent Python hashlib SHA-256 over the literal six-pair fixture JSON.
    assert_eq!(
        original.plan_sha256,
        "eba6b054f1b8057f27912a9adaff9de275e440cf8cdca6885df5e43c5f0a27e7"
    );
    let mut changed = original.clone();
    changed.plan.products[0].bins.pop();
    assert!(
        changed.validate().is_err(),
        "unchanged digest cannot mask bin removal"
    );
    changed.plan_sha256 = preview_plan_sha256(&changed.plan).expect("changed plan hash");
    assert_ne!(changed.plan_sha256, original.plan_sha256);
    assert!(
        changed
            .validate_against(&original.plan, &original.context)
            .is_err()
    );
    changed = original.clone();
    changed
        .plan
        .companions
        .push(crate::config::RustBinaryPreviewCompanion {
            id: "manifest".to_owned(),
            path: "manifest.json".to_owned(),
            asset_name: "manifest.json".to_owned(),
        });
    assert!(
        changed.validate().is_err(),
        "companions are part of the full digest"
    );
    assert_ne!(
        preview_plan_sha256(&changed.plan).expect("companion plan"),
        original.plan_sha256
    );
    changed = original.clone();
    changed.plan_sha256 = "B".repeat(64);
    assert!(changed.validate().is_err());
    changed = original;
    changed.schema = "velnor.preview-production.v2".to_owned();
    assert!(changed.validate().is_err());
}

#[test]
fn leaf_rejects_unknown_pairs_and_stable_plan_substitution() {
    let mut candidate = request();
    candidate.stage = PreviewProductionStage::Payload {
        pair: PreviewPayloadPair {
            product_id: "jackin-capsule".to_owned(),
            target: "aarch64-apple-darwin".to_owned(),
        },
    };
    assert!(candidate.validate().is_err());
    let mut stable = candidate.plan;
    stable.channel = crate::package_release::PackageReleaseChannel::Stable;
    stable.products.truncate(1);
    stable.products[0].bins.truncate(1);
    stable.products[0].targets.truncate(1);
    stable.products[0].format = crate::package_release::PackageReleasePayloadFormat::RawBinary;
    stable.validate().expect("otherwise valid stable plan");
    assert!(expected_preview_payload_pairs(&stable).is_err());
    let context = request().context;
    assert!(validate_preview_production_context(&stable, &context).is_err());
    assert!(preview_aggregate_artifact_name(&stable, &context).is_err());
}

#[test]
fn strict_parser_rejects_unknown_duplicate_noncanonical_and_oversized_json() {
    let bytes = request().canonical_bytes().expect("request bytes");
    let text = String::from_utf8(bytes.clone()).expect("UTF-8");
    for invalid in [
        format!(" {text}"),
        text.replacen(
            "\"schema\":\"velnor.preview-production.v1\"",
            "\"extra\":true,\"schema\":\"velnor.preview-production.v1\"",
            1,
        ),
        text.replacen(
            "\"base_version\":\"0.6.4\"",
            "\"base_version\":\"0.6.4\",\"matrix\":[]",
            1,
        ),
        text.replacen("\"run_id\":202", "\"run_id\":2.02e2", 1),
        text.replacen("\"run_attempt\":2", "\"run_attempt\":02", 1),
        text.replacen("\"run_id\":202", "\"run_id\":202,\"run_id\":202", 1),
        text.replacen("\"kind\":\"payload\"", "\"kind\":\"arbitrary\"", 1),
        text.replacen(
            "\"kind\":\"payload\"",
            "\"kind\":\"payload\",\"argv\":[]",
            1,
        ),
        text.replacen(
            "\"repository_id\":101",
            "\"repository_id\":101,\"verified\":true",
            1,
        ),
        text.replacen(
            "\"product_id\":\"jackin\"",
            "\"product_id\":\"jackin\",\"selector\":\"all\"",
            1,
        ),
    ] {
        assert_ne!(invalid.as_bytes(), bytes, "mutation must be exercised");
        assert!(
            parse_preview_production_request(invalid.as_bytes()).is_err(),
            "{invalid}"
        );
    }
    assert!(parse_preview_production_request(&[0xff]).is_err());
    assert!(
        parse_preview_production_request(&vec![b' '; MAX_PREVIEW_PRODUCTION_BYTES + 1]).is_err()
    );
}

#[test]
fn strict_assembly_parser_rejects_missing_digest_and_unknown_reference_fields() {
    let bytes = assembly().canonical_bytes().expect("assembly bytes");
    let text = String::from_utf8(bytes).expect("UTF-8");
    for invalid in [
        text.replacen(&format!(",\"sha256\":\"{}\"", "b".repeat(64)), "", 1),
        text.replacen("\"artifact_id\":100", "\"artifact_id\":-1", 1),
        text.replacen(
            "\"artifact_id\":100",
            "\"artifact_id\":18446744073709551616",
            1,
        ),
        text.replacen(
            "\"artifact_id\":100",
            "\"artifact_id\":100,\"verified\":true",
            1,
        ),
    ] {
        assert_ne!(invalid, text, "mutation must be exercised");
        assert!(parse_preview_production_request(invalid.as_bytes()).is_err());
    }
}

#[test]
fn artifact_name_binds_pair_run_and_attempt_without_name_lookup_authority() {
    let request = request();
    let pairs = expected_preview_payload_pairs(&request.plan).expect("pairs");
    let name = preview_payload_artifact_name(&request.plan, &pairs[0], &request.context)
        .expect("canonical locator");
    assert_eq!(
        name,
        "velnor-preview-payload-r202-a2-jackin-aarch64-apple-darwin"
    );
    let mut retry = request.context.clone();
    retry.run_attempt += 1;
    assert_ne!(
        name,
        preview_payload_artifact_name(&request.plan, &pairs[0], &retry).expect("retry")
    );
    assert_ne!(
        name,
        preview_payload_artifact_name(&request.plan, &pairs[1], &request.context)
            .expect("other leaf")
    );
    let mut unknown = pairs[0].clone();
    unknown.product_id = "../../escape".to_owned();
    assert!(preview_payload_artifact_name(&request.plan, &unknown, &request.context).is_err());
    assert!(preview_payload_job_id(&request.plan, &unknown).is_err());
}

#[test]
fn pair_expansion_has_its_own_bounded_preview_inventory() {
    let mut plan = request().plan;
    let template = plan.products[0].clone();
    plan.products = (0..32)
        .map(|index| {
            let mut product = template.clone();
            product.id = format!("product-{index:02}");
            product.package = product.id.clone();
            product
        })
        .collect();
    assert_eq!(
        expected_preview_payload_pairs(&plan)
            .expect("closed maximum")
            .len(),
        MAX_PREVIEW_PAYLOAD_PAIRS
    );
    let pairs = expected_preview_payload_pairs(&plan).expect("maximum inventory");
    let ids = pairs
        .iter()
        .map(|pair| preview_payload_job_id(&plan, pair).expect("bounded job"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 128);
    assert_eq!(ids.first().expect("first"), "preview-payload-000");
    assert_eq!(ids.last().expect("last"), "preview-payload-127");
    let mut extra = template;
    extra.id = "product-32".to_owned();
    extra.targets.truncate(1);
    extra.package = extra.id.clone();
    plan.products.push(extra);
    plan.validate()
        .expect("129 pairs remain valid full release inventory");
    assert_eq!(
        plan.products
            .iter()
            .map(|product| product.targets.len())
            .sum::<usize>(),
        129
    );
    assert!(expected_preview_payload_pairs(&plan).is_err());
}

#[test]
fn aggregate_locator_requires_valid_full_context_and_binds_current_run_attempt() {
    let request = request();
    validate_preview_production_context(&request.plan, &request.context).expect("syntax only");
    assert_eq!(
        preview_aggregate_artifact_name(&request.plan, &request.context).expect("locator"),
        "velnor-preview-aggregate-r202-a2"
    );
    let mut retry = request.context.clone();
    retry.run_attempt += 1;
    assert_eq!(
        preview_aggregate_artifact_name(&request.plan, &retry).expect("new attempt"),
        "velnor-preview-aggregate-r202-a3"
    );
    retry.run_id += 1;
    assert_eq!(
        preview_aggregate_artifact_name(&request.plan, &retry).expect("new run"),
        "velnor-preview-aggregate-r203-a3"
    );
    let mut invalid = request.plan;
    invalid.products.clear();
    assert!(validate_preview_production_context(&invalid, &retry).is_err());
    assert!(preview_aggregate_artifact_name(&invalid, &retry).is_err());
}
