use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const REPOSITORY: &str = "tailrocks/velnor-new";
const RELEASE_VERSION: &str = "0.1.0";
const MANIFEST_NAME: &str = "velnor-actions-release-manifest.json";
const LINUX_TARGET: &str = "x86_64-unknown-linux-gnu";
const MACOS_TARGET: &str = "aarch64-apple-darwin";
const LINUX_SHA: &str = "2e43e05be6002cfe3837d95b0501673bc2ab21f9d1ad7f97da87b427dcdd82b0";
const MACOS_SHA: &str = "f690c2a737e988a7d26b6e718daf2fc37f86ab07400a5e70d6bfa3e626658f58";
const LINUX_SIDECAR_SHA: &str = "d57170b4b32945ec812c9206e0fb24fbe89d2704fa44e362877d7fc04a70b248";
const MACOS_SIDECAR_SHA: &str = "73d6fb595617adc0d5ee5f3a399899e2f0d8dcbc312730710f4731fe76c3bba8";

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
    let manifest_size = fs::metadata(fixture.manifest())?.len();
    let mut final_records = fixture.records.clone();
    final_records.push(asset_record(
        MANIFEST_NAME,
        manifest_size,
        digest,
        Some(&asset_url(&fixture.tag, MANIFEST_NAME)),
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

fn write_local_assets(directory: &Path, version: &str) -> Result<(), Box<dyn Error>> {
    for (target, bytes, digest) in [
        (
            LINUX_TARGET,
            b"linux binary fixture\n".as_slice(),
            LINUX_SHA,
        ),
        (
            MACOS_TARGET,
            b"macos binary fixture\n".as_slice(),
            MACOS_SHA,
        ),
    ] {
        let name = format!("velnor-actions-{version}-{target}");
        fs::write(directory.join(&name), bytes)?;
        fs::write(
            directory.join(format!("{name}.sha256")),
            format!("{digest}  {name}\n"),
        )?;
    }
    Ok(())
}

fn asset_records(version: &str, tag: &str) -> Vec<String> {
    let mut records = Vec::new();
    for (target, binary_digest, sidecar_digest, binary_size, sidecar_size) in [
        (LINUX_TARGET, LINUX_SHA, LINUX_SIDECAR_SHA, 21, 112),
        (MACOS_TARGET, MACOS_SHA, MACOS_SIDECAR_SHA, 21, 108),
    ] {
        let name = format!("velnor-actions-{version}-{target}");
        records.push(asset_record(
            &name,
            binary_size,
            binary_digest,
            Some(&asset_url(tag, &name)),
        ));
        let sidecar = format!("{name}.sha256");
        records.push(asset_record(
            &sidecar,
            sidecar_size,
            sidecar_digest,
            Some(&asset_url(tag, &sidecar)),
        ));
    }
    records
}

fn asset_records_without_url(version: &str, tag: &str) -> Vec<String> {
    let mut records = asset_records(version, tag);
    let url_start = records[0]
        .find(",\"browser_download_url\"")
        .expect("fixture URL");
    let end = records[0].rfind('}').expect("fixture object");
    records[0].replace_range(url_start..end, "");
    records
}

fn asset_record(name: &str, size: u64, digest: &str, url: Option<&str>) -> String {
    let url = url.map_or_else(String::new, |value| {
        format!(",\"browser_download_url\":\"{value}\"")
    });
    format!(
        "{{\"name\":\"{name}\",\"state\":\"uploaded\",\"size\":{size},\"digest\":\"sha256:{digest}\"{url}}}"
    )
}

fn release_json(draft: bool, immutable: bool, tag: &str, assets: &[String]) -> String {
    format!(
        "{{\"tag_name\":\"{tag}\",\"target_commitish\":\"main\",\"draft\":{draft},\"prerelease\":false,\"immutable\":{immutable},\"assets\":[{}]}}",
        assets.join(",")
    )
}

fn asset_url(tag: &str, asset: &str) -> String {
    format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{asset}")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output, reason: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected {reason}: {stderr}");
    assert!(stderr.contains(reason), "expected {reason}: {stderr}");
}

fn output_text(output: &Output) -> Result<String, Box<dyn Error>> {
    Ok(String::from_utf8(output.stdout.clone())?)
}

#[cfg(test)]
#[path = "schema2_generator_release_url_tests.rs"]
mod url_tests;

#[cfg(unix)]
#[path = "schema2_generator_release_publisher_tests.rs"]
mod publisher_tests;
