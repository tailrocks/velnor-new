use super::{
    Fixture, LINUX_TARGET, MANIFEST_NAME, RELEASE_VERSION, assert_failure, assert_success,
    asset_record, asset_url, output_text, release_json, run_helper,
};
use std::error::Error;
use std::fs;

const MANIFEST_CHECKSUM_NAME: &str = "velnor-actions-release-manifest.json.sha256";
const BAD_SIDECAR_SHA: &str = "20c131057bedc10ae24bdae230efd695d3aebc91d453ce9e30fc18c61027e121";
const TAMPERED_MANIFEST_SHA: &str =
    "ca3d163bab055381827226140568f3bef7eaac187cebd76878e0b63e9e442356";

#[test]
fn reject_noncanonical_browser_download_urls() -> Result<(), Box<dyn Error>> {
    for (name, replacement) in [
        ("leading-space", " https://"),
        ("uppercase-scheme", "HTTPS://"),
    ] {
        let fixture = Fixture::new(name)?;
        let mut records = fixture.records.clone();
        records[0] = records[0].replacen("https://", replacement, 1);
        let release = release_json(true, false, &fixture.tag, &records);
        let output = run_helper(
            &fixture,
            "create",
            &release,
            RELEASE_VERSION,
            &fixture.commit,
        );
        assert_failure(&output, "noncanonical_browser_download_url");
        assert!(!fixture.manifest().exists());
    }
    Ok(())
}

#[test]
fn candidate_must_be_draft_and_published_release_must_be_immutable() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("not-draft")?;
    let release = release_json(false, true, &fixture.tag, &fixture.records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "candidate_release_not_draft");

    let fixture = Fixture::new("mutable-published")?;
    let draft = release_json(true, false, &fixture.tag, &fixture.records);
    let output = run_helper(&fixture, "create", &draft, RELEASE_VERSION, &fixture.commit);
    assert_success(&output);
    let helper_output = output_text(&output)?;
    let manifest_digest = helper_output
        .lines()
        .find_map(|line| line.strip_prefix("release_manifest_sha256="))
        .ok_or("missing manifest digest")?
        .to_owned();
    let manifest_size = fs::metadata(fixture.manifest())?.len();
    let mut records = fixture.records.clone();
    records.push(asset_record(
        MANIFEST_NAME,
        manifest_size,
        &manifest_digest,
        Some(&asset_url(&fixture.tag, MANIFEST_NAME)),
    ));
    let release = release_json(false, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "verify",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "published_release_not_immutable");
    Ok(())
}

#[test]
fn reject_bad_sidecar_and_tampered_manifest() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("bad-sidecar")?;
    let binary = format!("velnor-actions-{RELEASE_VERSION}-{LINUX_TARGET}");
    let sidecar = format!("{binary}.sha256");
    fs::write(fixture.asset_dir.join(&sidecar), b"not a checksum\n")?;
    let mut records = fixture.records.clone();
    records[1] = asset_record(
        &sidecar,
        15,
        BAD_SIDECAR_SHA,
        Some(&asset_url(&fixture.tag, &sidecar)),
    );
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "release_sidecar_mismatch");

    let fixture = Fixture::new("tampered-manifest")?;
    let draft = release_json(true, false, &fixture.tag, &fixture.records);
    let output = run_helper(&fixture, "create", &draft, RELEASE_VERSION, &fixture.commit);
    assert_success(&output);
    let helper_output = output_text(&output)?;
    let checksum_digest = helper_output
        .lines()
        .find_map(|line| line.strip_prefix("release_manifest_checksum_sha256="))
        .ok_or("missing manifest checksum digest")?
        .to_owned();
    let checksum_size = fs::metadata(fixture.asset_dir.join(MANIFEST_CHECKSUM_NAME))?.len();
    fs::write(fixture.manifest(), b"{}\n")?;
    let mut records = fixture.records.clone();
    records.push(asset_record(
        MANIFEST_NAME,
        3,
        TAMPERED_MANIFEST_SHA,
        Some(&asset_url(&fixture.tag, MANIFEST_NAME)),
    ));
    records.push(asset_record(
        MANIFEST_CHECKSUM_NAME,
        checksum_size,
        &checksum_digest,
        Some(&asset_url(&fixture.tag, MANIFEST_CHECKSUM_NAME)),
    ));
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "verify-draft",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "release_manifest_sidecar_mismatch");
    Ok(())
}
