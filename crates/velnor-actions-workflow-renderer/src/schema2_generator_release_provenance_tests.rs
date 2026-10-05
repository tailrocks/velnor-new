use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "schema2_generator_release_api_fixtures.rs"]
mod api_fixtures;
use self::api_fixtures::{
    LINUX_SHA, LINUX_TARGET, MACOS_SHA, MACOS_TARGET, MANIFEST_CHECKSUM_NAME, MANIFEST_NAME,
    RELEASE_VERSION, REPOSITORY, assert_failure, assert_success, asset_record, asset_records,
    asset_records_without_url, asset_url, output_text, release_json, write_local_assets,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-release-provenance-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

struct Fixture {
    scratch: Scratch,
    asset_dir: PathBuf,
    records: Vec<String>,
    commit: String,
    tag: String,
}

impl Fixture {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let scratch = Scratch::new(name)?;
        let asset_dir = scratch.path().join("assets");
        fs::create_dir_all(&asset_dir)?;
        let commit = "ab".repeat(20);
        let tag = format!("generator-{commit}");
        let records = asset_records(RELEASE_VERSION, &tag);
        write_local_assets(&asset_dir, RELEASE_VERSION)?;
        Ok(Self {
            scratch,
            asset_dir,
            records,
            commit,
            tag,
        })
    }

    fn manifest(&self) -> PathBuf {
        self.asset_dir.join(MANIFEST_NAME)
    }

    fn release_file(&self, name: &str, text: &str) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.scratch.path().join(name);
        fs::write(&path, text)?;
        Ok(path)
    }
}

#[test]
fn manifest_uses_actual_api_urls_and_verifies_the_published_release() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new("actual-url")?;
    let initial = release_json(true, false, &fixture.tag, &fixture.records);
    let output = run_helper(
        &fixture,
        "create",
        &initial,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_success(&output);
    let manifest = fs::read_to_string(fixture.manifest())?;
    for target in [LINUX_TARGET, MACOS_TARGET] {
        let name = format!("velnor-actions-{RELEASE_VERSION}-{target}");
        let actual_url = asset_url(&fixture.tag, &name);
        assert!(manifest.contains(&format!("\"artifact\":\"{actual_url}\"")));
    }
    assert!(manifest.contains(&format!("\"commit\":\"{}\"", fixture.commit)));
    assert!(manifest.contains(&format!("\"version\":\"{RELEASE_VERSION}\"")));
    assert!(manifest.contains(&format!("\"repository\":\"{REPOSITORY}\"")));

    let digest = output_text(&output)?
        .lines()
        .find_map(|line| line.strip_prefix("release_manifest_sha256="))
        .ok_or("missing manifest digest")?;
    let checksum_digest = output_text(&output)?
        .lines()
        .find_map(|line| line.strip_prefix("release_manifest_checksum_sha256="))
        .ok_or("missing manifest checksum digest")?;
    let manifest_size = fs::metadata(fixture.manifest())?.len();
    let checksum_size = fs::metadata(fixture.asset_dir.join(MANIFEST_CHECKSUM_NAME))?.len();
    let mut final_records = fixture.records.clone();
    final_records.push(asset_record(
        MANIFEST_NAME,
        manifest_size,
        digest,
        Some(&asset_url(&fixture.tag, MANIFEST_NAME)),
    ));
    final_records.push(asset_record(
        MANIFEST_CHECKSUM_NAME,
        checksum_size,
        checksum_digest,
        Some(&asset_url(&fixture.tag, MANIFEST_CHECKSUM_NAME)),
    ));
    let draft = release_json(true, false, &fixture.tag, &final_records);
    let output = run_helper(
        &fixture,
        "verify-draft",
        &draft,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_success(&output);
    let published = release_json(false, true, &fixture.tag, &final_records);
    let output = run_helper(
        &fixture,
        "verify",
        &published,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_success(&output);
    Ok(())
}

#[test]
fn missing_or_synthesized_api_urls_fail_without_a_manifest() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("missing-url")?;
    let records = asset_records_without_url(RELEASE_VERSION, &fixture.tag);
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "missing_browser_download_url");
    assert!(!fixture.manifest().exists());

    let fixture = Fixture::new("unbound-url")?;
    let mut records = fixture.records.clone();
    records[0] = records[0].replace(
        &fixture.tag,
        "generator-ffffffffffffffffffffffffffffffffffffffff",
    );
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "unbound_browser_download_url");
    assert!(!fixture.manifest().exists());
    Ok(())
}

#[test]
fn source_target_version_digest_and_asset_path_mismatches_fail() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("wrong-source")?;
    let release = release_json(true, false, &fixture.tag, &fixture.records);
    let output = run_helper_with_tag_commit(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &"cd".repeat(20),
    );
    assert_failure(&output, "source_tag_binding_mismatch");

    let fixture = Fixture::new("wrong-target")?;
    let mut records = fixture.records.clone();
    records[2] = records[2].replace(MACOS_TARGET, "x86_64-apple-darwin");
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "release_asset_set_mismatch");

    let fixture = Fixture::new("wrong-version")?;
    let release = release_json(true, false, &fixture.tag, &fixture.records);
    let output = run_helper(&fixture, "create", &release, "0.1.1", &fixture.commit);
    assert_failure(&output, "release_asset_set_mismatch");

    let fixture = Fixture::new("wrong-digest")?;
    let mut records = fixture.records.clone();
    records[0] = records[0].replace(LINUX_SHA, &"0".repeat(64));
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "release_asset_digest_mismatch");

    let fixture = Fixture::new("wrong-state")?;
    let mut records = fixture.records.clone();
    records[0] = records[0].replace("\"state\":\"uploaded\"", "\"state\":\"starter\"");
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "release_asset_state_or_size_mismatch");

    let fixture = Fixture::new("unsafe-name")?;
    let mut records = fixture.records.clone();
    records[0] = records[0].replace(
        &format!("velnor-actions-{RELEASE_VERSION}-{LINUX_TARGET}"),
        "../escape",
    );
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "release_asset_set_mismatch");
    Ok(())
}

#[test]
fn publisher_uploads_acceptance_metadata_only_after_release_verification()
-> Result<(), Box<dyn Error>> {
    use std::collections::BTreeSet;
    use velnor_actions_contract::RoutingWorkflow;

    let request = super::Schema2WorkflowRequest {
        version: RELEASE_VERSION.to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: super::Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
    };
    let workflow = crate::yaml::render_yaml(&super::generator_release(&request)?);
    let publication = workflow
        .find("name: Publish GitHub release")
        .ok_or("missing publication step")?;
    let acceptance = workflow
        .find("name: Upload verified release metadata")
        .ok_or("missing acceptance artifact step")?;
    assert!(publication < acceptance);
    assert!(workflow.contains("actions: write"));
    assert!(workflow.contains("contents: write"));
    assert!(workflow.contains("velnor-actions-release-manifest.json.sha256"));
    assert!(workflow.contains("velnor-actions-release-acceptance.json"));
    assert!(workflow.contains("if-no-files-found: error"));
    assert!(workflow.contains("retention-days: 14"));
    Ok(())
}

fn run_helper(
    fixture: &Fixture,
    mode: &str,
    release: &str,
    version: &str,
    tag_commit: &str,
) -> Output {
    run_helper_with_tag_commit(fixture, mode, release, version, tag_commit)
}

fn run_helper_with_tag_commit(
    fixture: &Fixture,
    mode: &str,
    release: &str,
    version: &str,
    tag_commit: &str,
) -> Output {
    let release_path = fixture
        .release_file("release.json", release)
        .expect("write release fixture");
    Command::new("python3")
        .arg(helper_path())
        .args([
            "--mode",
            mode,
            "--release-json",
            release_path.to_str().expect("UTF-8 fixture path"),
            "--asset-dir",
            fixture.asset_dir.to_str().expect("UTF-8 asset path"),
            "--manifest",
            fixture.manifest().to_str().expect("UTF-8 manifest path"),
            "--version",
            version,
            "--repository",
            REPOSITORY,
            "--commit",
            &fixture.commit,
            "--tag",
            &fixture.tag,
            "--tag-commit",
            tag_commit,
        ])
        .output()
        .expect("run fixture-only manifest helper")
}

fn helper_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/create-release-manifest.py")
}

#[cfg(test)]
#[path = "schema2_generator_release_url_tests.rs"]
mod url_tests;

#[cfg(unix)]
#[path = "schema2_generator_release_publisher_tests.rs"]
mod publisher_tests;
