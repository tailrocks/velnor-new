use super::*;
use velnor_actions_contract::{
    GeneratorReleasePlan, GeneratorReleaseSourceBinding, RoutingWorkflow,
};

#[test]
fn manifest_uses_actual_api_urls_and_verifies_the_published_release() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new("actual-url")?;
    let (source_plan, final_records) = create_manifest_and_final_records(&fixture)?;
    assert_rendered_release_contract(&source_plan)?;
    verify_release_states(&fixture, &final_records);
    Ok(())
}

fn create_manifest_and_final_records(
    fixture: &Fixture,
) -> Result<(GeneratorReleasePlan, Vec<String>), Box<dyn Error>> {
    let binding = GeneratorReleaseSourceBinding::for_current_workflow(RELEASE_VERSION)?;
    let source_plan = binding.bind(&fixture.commit)?;
    assert_eq!(source_plan.tag(), fixture.tag);
    assert_eq!(source_plan.source_sha(), fixture.commit);
    let initial = release_json(true, false, &fixture.tag, &fixture.records);
    let output = run_helper(
        fixture,
        "create",
        &initial,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_success(&output);
    let manifest = fs::read_to_string(fixture.manifest())?;
    assert_observed_asset_urls(&manifest, fixture);
    assert_source_fields(&manifest, fixture);
    let (digest, checksum_digest) = manifest_digests(&output)?;
    let final_records = final_manifest_records(fixture, &digest, &checksum_digest)?;
    assert_eq!(
        asset_names(&final_records)?,
        source_plan.final_asset_names()
    );
    Ok((source_plan, final_records))
}

fn assert_observed_asset_urls(manifest: &str, fixture: &Fixture) {
    for target in [LINUX_TARGET, MACOS_TARGET] {
        let name = format!("velnor-actions-{RELEASE_VERSION}-{target}");
        let url = asset_url(&fixture.tag, &name);
        assert!(manifest.contains(&format!("\"artifact\":\"{url}\"")));
    }
}

fn assert_source_fields(manifest: &str, fixture: &Fixture) {
    assert!(manifest.contains(&format!("\"commit\":\"{}\"", fixture.commit)));
    assert!(manifest.contains(&format!("\"version\":\"{RELEASE_VERSION}\"")));
    assert!(manifest.contains(&format!("\"repository\":\"{REPOSITORY}\"")));
}

fn manifest_digests(output: &Output) -> Result<(String, String), Box<dyn Error>> {
    let text = output_text(output)?;
    let digest = output_digest(&text, "release_manifest_sha256=")?;
    let checksum = output_digest(&text, "release_manifest_checksum_sha256=")?;
    Ok((digest, checksum))
}

fn output_digest(text: &str, prefix: &str) -> Result<String, Box<dyn Error>> {
    text.lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(str::to_owned)
        .ok_or_else(|| format!("missing helper output {prefix}").into())
}

fn final_manifest_records(
    fixture: &Fixture,
    digest: &str,
    checksum_digest: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    let manifest_size = fs::metadata(fixture.manifest())?.len();
    let checksum_path = fixture.asset_dir.join(MANIFEST_CHECKSUM_NAME);
    let checksum_size = fs::metadata(checksum_path)?.len();
    let mut records = fixture.records.clone();
    records.push(asset_record(
        MANIFEST_NAME,
        manifest_size,
        digest,
        Some(&asset_url(&fixture.tag, MANIFEST_NAME)),
    ));
    records.push(asset_record(
        MANIFEST_CHECKSUM_NAME,
        checksum_size,
        checksum_digest,
        Some(&asset_url(&fixture.tag, MANIFEST_CHECKSUM_NAME)),
    ));
    Ok(records)
}

fn asset_names(records: &[String]) -> Result<Vec<String>, Box<dyn Error>> {
    records.iter().map(|record| record_name(record)).collect()
}

fn assert_rendered_release_contract(
    source_plan: &GeneratorReleasePlan,
) -> Result<(), Box<dyn Error>> {
    let request = super::Schema2WorkflowRequest {
        version: RELEASE_VERSION.to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: super::Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: std::collections::BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
    };
    let workflow = crate::yaml::render_yaml(&super::generator_release(&request)?);
    for expected in [
        "VELNOR_RELEASE_SOURCE_SHA: ${{ github.sha }}".to_owned(),
        source_plan.final_asset_names()[0].clone(),
        source_plan.final_asset_names()[3].clone(),
        "--version '0.1.0'".to_owned(),
        "--target x86_64-unknown-linux-gnu".to_owned(),
        "--target aarch64-apple-darwin".to_owned(),
    ] {
        assert!(
            workflow.contains(&expected),
            "missing rendered contract: {expected}"
        );
    }
    Ok(())
}

fn verify_release_states(fixture: &Fixture, records: &[String]) {
    for (mode, draft, immutable) in [("verify-draft", true, false), ("verify", false, true)] {
        let release = release_json(draft, immutable, &fixture.tag, records);
        let output = run_helper(fixture, mode, &release, RELEASE_VERSION, &fixture.commit);
        assert_success(&output);
    }
}

fn record_name(record: &str) -> Result<String, Box<dyn Error>> {
    Ok(record
        .split_once("\"name\":\"")
        .ok_or("missing release asset name")?
        .1
        .split_once('"')
        .ok_or("unterminated release asset name")?
        .0
        .to_owned())
}

#[test]
fn publisher_uploads_acceptance_metadata_only_after_release_verification()
-> Result<(), Box<dyn Error>> {
    let request = super::Schema2WorkflowRequest {
        version: RELEASE_VERSION.to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: super::Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: std::collections::BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
    };
    let workflow = crate::yaml::render_yaml(&super::generator_release(&request)?);
    let publish = workflow
        .find("name: Publish GitHub release")
        .ok_or("missing publication step")?;
    let acceptance = workflow
        .find("name: Upload verified release metadata")
        .ok_or("missing acceptance artifact step")?;
    assert!(publish < acceptance);
    for expected in [
        "actions: write",
        "contents: write",
        "velnor-actions-release-manifest.json.sha256",
        "velnor-actions-release-acceptance.json",
        "if-no-files-found: error",
        "retention-days: 14",
    ] {
        assert!(
            workflow.contains(expected),
            "missing publisher contract: {expected}"
        );
    }
    Ok(())
}
