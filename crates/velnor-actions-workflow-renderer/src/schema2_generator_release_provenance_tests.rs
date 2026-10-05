use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
#[path = "schema2_generator_release_api_fixtures.rs"]
mod api_fixtures;
use self::api_fixtures::{
    LINUX_SHA, LINUX_TARGET, MACOS_ARM64_TARGET, MACOS_X86_64_TARGET, MANIFEST_CHECKSUM_NAME,
    MANIFEST_NAME, RELEASE_VERSION, REPOSITORY, TARGET_FIXTURES, assert_failure, assert_success,
    asset_record, asset_records, asset_records_without_url, asset_url, output_text, release_json,
    write_local_assets,
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
    for record in &mut records {
        if record.contains(MACOS_X86_64_TARGET) {
            *record = record.replace(MACOS_X86_64_TARGET, "x86_64-apple-darwin-invalid");
        }
    }
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "release_asset_set_mismatch");

    let fixture = Fixture::new("missing-third-target")?;
    let mut records = fixture.records.clone();
    records.retain(|record| !record.contains(MACOS_X86_64_TARGET));
    let release = release_json(true, false, &fixture.tag, &records);
    let output = run_helper(
        &fixture,
        "create",
        &release,
        RELEASE_VERSION,
        &fixture.commit,
    );
    assert_failure(&output, "release_asset_set_mismatch");

    assert!(
        TARGET_FIXTURES
            .iter()
            .any(|(target, _, _, _, _)| *target == LINUX_TARGET)
    );
    assert!(
        TARGET_FIXTURES
            .iter()
            .any(|(target, _, _, _, _)| *target == MACOS_ARM64_TARGET)
    );
    assert!(
        TARGET_FIXTURES
            .iter()
            .any(|(target, _, _, _, _)| *target == MACOS_X86_64_TARGET)
    );

    let fixture = Fixture::new("wrong-version")?;
    let release = release_json(true, false, &fixture.tag, &fixture.records);
    let output = run_helper(&fixture, "create", &release, "0.1.1", &fixture.commit);
    assert_failure(&output, "release_asset_set_mismatch");

    let fixture = Fixture::new("noncanonical-version")?;
    let release = release_json(true, false, &fixture.tag, &fixture.records);
    let output = run_helper(&fixture, "create", &release, "01.0.0", &fixture.commit);
    assert_failure(&output, "malformed_version");
    assert!(!fixture.manifest().exists());

    let fixture = Fixture::new("oversized-version")?;
    let release = release_json(true, false, &fixture.tag, &fixture.records);
    let oversized_version = format!("{}.0.0", "9".repeat(5000));
    let output = run_helper(
        &fixture,
        "create",
        &release,
        &oversized_version,
        &fixture.commit,
    );
    assert_failure(&output, "malformed_version");
    assert!(!fixture.manifest().exists());

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

#[path = "schema2_generator_release_contract_tests.rs"]
mod contract_tests;
