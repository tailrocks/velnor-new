use super::*;

#[test]
fn leaf_rejects_unknown_pairs_and_mixed_channel_context() {
    let mut candidate = request();
    candidate.stage = ReleaseProductionStage::Payload {
        pair: ReleasePayloadPair {
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
    stable.validate().expect("otherwise valid stable plan");
    assert_eq!(
        expected_release_payload_pairs(&stable)
            .expect("stable pair")
            .len(),
        1
    );
    let context = request().context;
    assert!(validate_release_production_context(&stable, &context).is_err());
    assert!(release_aggregate_artifact_name(&stable, &context).is_err());
}

#[test]
fn strict_parser_rejects_unknown_duplicate_noncanonical_and_oversized_json() {
    let bytes = request().canonical_bytes().expect("request bytes");
    let text = String::from_utf8(bytes.clone()).expect("UTF-8");
    for invalid in [
        format!(" {text}"),
        text.replacen(
            "\"schema\":\"velnor.release-production.v1\"",
            "\"extra\":true,\"schema\":\"velnor.release-production.v1\"",
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
            parse_release_production_request(invalid.as_bytes()).is_err(),
            "{invalid}"
        );
    }
    assert!(parse_release_production_request(&[0xff]).is_err());
    assert!(
        parse_release_production_request(&vec![b' '; MAX_RELEASE_PRODUCTION_BYTES + 1]).is_err()
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
        assert!(parse_release_production_request(invalid.as_bytes()).is_err());
    }
}

#[test]
fn artifact_name_binds_pair_run_and_attempt_without_name_lookup_authority() {
    let request = request();
    let pairs = expected_release_payload_pairs(&request.plan).expect("pairs");
    let name = release_payload_artifact_name(&request.plan, &pairs[0], &request.context)
        .expect("canonical locator");
    assert_eq!(
        name,
        "velnor-release-payload-r202-a2-jackin-aarch64-apple-darwin"
    );
    let mut retry = request.context.clone();
    retry.run_attempt += 1;
    assert_ne!(
        name,
        release_payload_artifact_name(&request.plan, &pairs[0], &retry).expect("retry")
    );
    assert_ne!(
        name,
        release_payload_artifact_name(&request.plan, &pairs[1], &request.context)
            .expect("other leaf")
    );
    let mut unknown = pairs[0].clone();
    unknown.product_id = "../../escape".to_owned();
    assert!(release_payload_artifact_name(&request.plan, &unknown, &request.context).is_err());
    assert!(release_payload_job_id(&request.plan, &unknown).is_err());
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
        expected_release_payload_pairs(&plan)
            .expect("closed maximum")
            .len(),
        MAX_RELEASE_PAYLOAD_PAIRS
    );
    let pairs = expected_release_payload_pairs(&plan).expect("maximum inventory");
    let ids = pairs
        .iter()
        .map(|pair| release_payload_job_id(&plan, pair).expect("bounded job"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids.first().expect("one preview job"), "preview-payload");
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
    assert!(expected_release_payload_pairs(&plan).is_err());
}

#[test]
fn aggregate_locator_requires_valid_full_context_and_binds_current_run_attempt() {
    let request = request();
    validate_release_production_context(&request.plan, &request.context).expect("syntax only");
    assert_eq!(
        release_aggregate_artifact_name(&request.plan, &request.context).expect("locator"),
        "velnor-release-aggregate-r202-a2"
    );
    let mut retry = request.context.clone();
    retry.run_attempt += 1;
    assert_eq!(
        release_aggregate_artifact_name(&request.plan, &retry).expect("new attempt"),
        "velnor-release-aggregate-r202-a3"
    );
    retry.run_id += 1;
    assert_eq!(
        release_aggregate_artifact_name(&request.plan, &retry).expect("new run"),
        "velnor-release-aggregate-r203-a3"
    );
    let mut invalid = request.plan;
    invalid.products.clear();
    assert!(validate_release_production_context(&invalid, &retry).is_err());
    assert!(release_aggregate_artifact_name(&invalid, &retry).is_err());
}
