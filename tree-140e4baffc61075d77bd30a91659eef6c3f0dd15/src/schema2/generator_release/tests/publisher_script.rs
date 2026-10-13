use super::super::{assets, manifest};
use super::test_pins;
use super::{REPOSITORY, SOURCE_SHA};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct Scratch(pub(super) PathBuf);

impl Scratch {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-generator-publish-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Failure {
    None,
    WrongSidecarName,
    MissingBinary,
    StaleManifest,
    UnauthorizedTag,
    ForbiddenTag,
    TransientTag,
    WrongDraftReleaseId,
    WrongDraftDigest,
    WrongDraftUrl,
    WrongDraftSize,
    WrongDraftInventory,
    ChangedReleaseId,
    WrongPublishedDigest,
    MutablePublished,
}

#[test]
fn complete_publish_script_uses_parent_tag_and_checks_all_assets() -> Result<(), Box<dyn Error>> {
    for case in [
        Failure::None,
        Failure::WrongSidecarName,
        Failure::MissingBinary,
        Failure::StaleManifest,
        Failure::UnauthorizedTag,
        Failure::ForbiddenTag,
        Failure::TransientTag,
        Failure::WrongDraftReleaseId,
        Failure::WrongDraftDigest,
        Failure::WrongDraftUrl,
        Failure::WrongDraftSize,
        Failure::WrongDraftInventory,
        Failure::ChangedReleaseId,
        Failure::WrongPublishedDigest,
        Failure::MutablePublished,
    ] {
        run_publish_case(case)?;
    }
    Ok(())
}

fn run_publish_case(case: Failure) -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    copy_release_helpers(&scratch.0)?;
    write_candidate_records(&scratch.0, case)?;
    write_attestation_files(&scratch.0)?;
    create_candidate_manifest(&scratch.0, case)?;
    let draft_json = release_json(&scratch.0, case, true)?;
    fs::write(scratch.0.join("draft-release.json"), draft_json)?;
    let published_json = release_json(&scratch.0, case, false)?;
    fs::write(scratch.0.join("published-release.json"), published_json)?;
    install_mock_gh(&scratch.0)?;
    let script = scratch.0.join("publish.sh");
    fs::write(&script, manifest::publish_script(&test_pins()))?;
    let gh_function = super::super::workflow_steps::gh_function(&test_pins().gh_argv)?;
    let output = run_publish_command(&scratch.0, case, &script, &gh_function)?;
    assert_publish_result(&scratch.0, case, &output)?;
    super::cli_tests::assert_pinned_publish_calls(&scratch.0, case)
}

fn create_candidate_manifest(root: &Path, case: Failure) -> Result<(), Box<dyn Error>> {
    let manifest_status = Command::new("bash")
        .args([
            "scripts/generator-release/create-release-manifest.sh",
            "0.1.1",
            REPOSITORY,
            "1.98.1",
            "1.21.1",
        ])
        .current_dir(root)
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("GITHUB_SHA", SOURCE_SHA)
        .status()?;
    let invalid_manifest = matches!(case, Failure::WrongSidecarName | Failure::MissingBinary);
    if invalid_manifest {
        assert!(!manifest_status.success(), "accepted {case:?}");
        fs::create_dir_all(root.join("manifest-assets"))?;
        fs::write(
            root.join("manifest-assets/release-manifest.json"),
            b"untrusted fixture\n",
        )?;
    } else {
        assert!(manifest_status.success(), "manifest failed for {case:?}");
        fs::create_dir_all(root.join("manifest-assets"))?;
        fs::copy(
            root.join("release-manifest.json"),
            root.join("manifest-assets/release-manifest.json"),
        )?;
    }
    if case == Failure::StaleManifest {
        fs::write(
            root.join("manifest-assets/release-manifest.json"),
            b"stale manifest\n",
        )?;
    }
    Ok(())
}

fn run_publish_command(
    root: &Path,
    case: Failure,
    script: &Path,
    gh_function: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(format!("{gh_function}\n{}", fs::read_to_string(script)?))
        .current_dir(root)
        .env("PATH", path_with_mock_gh(root)?)
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("GITHUB_SHA", SOURCE_SHA)
        .env("GITHUB_WORKFLOW_SHA", SOURCE_SHA)
        .env("GITHUB_RUN_ID", "987654321")
        .env("GITHUB_RUN_ATTEMPT", "2")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GH_PREFLIGHT", preflight_mode(case))
        .env("GH_CASE", format!("{case:?}"))
        .env("GH_DRAFT_JSON", root.join("draft-release.json"))
        .env("GH_RELEASE_JSON", root.join("published-release.json"))
        .env("GH_STATE", root.join("release-state"))
        .env("GH_CREATE_TAG", root.join("created-tag"))
        .env("GH_ASSET_ARGS", root.join("asset-args"))
        .env("GH_PATCH_LOG", root.join("patch-log"))
        .env("GH_CALLS", root.join("gh-calls"))
        .env("MISE_CALLS", root.join("mise-calls"))
        .env("MOCK_GH", root.join("mock-bin/gh"));
    super::cli_tests::isolate_gh_environment(&mut command, root)?;
    Ok(command.output()?)
}

fn assert_publish_result(
    root: &Path,
    case: Failure,
    output: &std::process::Output,
) -> Result<(), Box<dyn Error>> {
    let should_succeed = case == Failure::None;
    let calls = fs::read_to_string(root.join("gh-calls")).unwrap_or_default();
    let mise_calls = fs::read_to_string(root.join("mise-calls")).unwrap_or_default();
    assert_eq!(
        output.status.success(),
        should_succeed,
        "case {case:?}: {}{}\nGitHub calls:\n{calls}\nMise calls:\n{mise_calls}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let create_is_expected = matches!(
        case,
        Failure::None
            | Failure::WrongDraftReleaseId
            | Failure::WrongDraftDigest
            | Failure::WrongDraftUrl
            | Failure::WrongDraftSize
            | Failure::WrongDraftInventory
            | Failure::ChangedReleaseId
            | Failure::WrongPublishedDigest
            | Failure::MutablePublished
    );
    if create_is_expected {
        assert_eq!(fs::read_to_string(root.join("created-tag"))?, "v0.1.1\n");
        assert_eq!(
            fs::read_to_string(root.join("asset-args"))?,
            expected_asset_args()
        );
        if case == Failure::None {
            assert!(
                root.join("release-accepted/release-manifest.json")
                    .is_file()
            );
            assert!(
                root.join("release-accepted/release-acceptance.json")
                    .is_file()
            );
            super::publisher_receipt_tests::assert_success_receipt(root)?;
        } else {
            assert!(!root.join("release-accepted").exists());
        }
    } else {
        assert_no_release_created(root);
    }
    Ok(())
}

fn copy_release_helpers(root: &Path) -> Result<(), Box<dyn Error>> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let target = root.join("scripts/generator-release");
    fs::create_dir_all(&target)?;
    for name in ["create-release-manifest.sh", "preflight-release-tag.sh"] {
        fs::copy(
            repo.join("scripts/generator-release").join(name),
            target.join(name),
        )?;
    }
    Ok(())
}

fn write_candidate_records(root: &Path, case: Failure) -> Result<(), Box<dyn Error>> {
    let products = [assets::LINUX, assets::MACOS_ARM64, assets::MACOS_X86_64];
    for (index, product) in products.into_iter().enumerate() {
        let directory = root.join(product.directory);
        fs::create_dir_all(&directory)?;
        let binary = directory.join(product.binary);
        if !(case == Failure::MissingBinary && index == 2) {
            fs::write(
                &binary,
                format!("candidate bytes for {}\n", product.target.triple()),
            )?;
        }
        let digest = if binary.exists() {
            sha256(&binary)?
        } else {
            "0".repeat(64)
        };
        let sidecar_name = if case == Failure::WrongSidecarName && index == 0 {
            "unrelated-binary"
        } else {
            product.binary
        };
        fs::write(
            directory.join(product.sidecar),
            format!("{digest}  {sidecar_name}\n"),
        )?;
        fs::write(
            directory.join(product.provenance),
            format!(
                "{{\"schema\":1,\"version\":\"0.1.1\",\"repository\":\"{REPOSITORY}\",\"commit\":\"{SOURCE_SHA}\",\"target\":\"{}\",\"asset\":\"{}\",\"sha256\":\"{digest}\",\"toolchain\":{{\"rust\":\"1.98.1\",\"mr-boxington\":\"1.21.1\"}}}}\n",
                product.target.triple(),
                product.binary
            ),
        )?;
    }
    Ok(())
}

pub(super) fn write_attestation_files(root: &Path) -> Result<(), Box<dyn Error>> {
    for path in manifest::release_asset_paths().split_whitespace() {
        if path.starts_with("release-attestations/") {
            let target = root.join(path);
            fs::create_dir_all(target.parent().ok_or("missing attestation parent")?)?;
            fs::write(target, b"signed bundle fixture\n")?;
        }
    }
    Ok(())
}

fn release_json(root: &Path, case: Failure, draft: bool) -> Result<String, Box<dyn Error>> {
    let mut rows = Vec::new();
    for (index, path) in manifest::release_asset_paths()
        .split_whitespace()
        .enumerate()
    {
        if draft && case == Failure::WrongDraftInventory && index == 19 {
            continue;
        }
        let asset = root.join(path);
        if !asset.exists() {
            continue;
        }
        let mut digest = sha256(&asset)?;
        let wrong_digest = matches!(
            (draft, case),
            (true, Failure::WrongDraftDigest) | (false, Failure::WrongPublishedDigest)
        );
        if wrong_digest && index == 0 {
            digest = "0".repeat(64);
        }
        let name = Path::new(path)
            .file_name()
            .ok_or("release asset has no basename")?
            .to_str()
            .ok_or("release asset basename is not UTF-8")?;
        let size = fs::metadata(&asset)?.len()
            + u64::from(draft && case == Failure::WrongDraftSize && index == 0);
        let url = if draft && case == Failure::WrongDraftUrl && index == 0 {
            "https://github.com/untrusted/releases/download/v0.1.1/asset".to_owned()
        } else {
            format!("https://github.com/{REPOSITORY}/releases/download/v0.1.1/{name}")
        };
        rows.push(format!(
            "{{\"name\":\"{name}\",\"state\":\"uploaded\",\"browser_download_url\":\"{url}\",\"digest\":\"sha256:{digest}\",\"size\":{size}}}"
        ));
    }
    let release_id = if (draft && case == Failure::WrongDraftReleaseId)
        || (!draft && case == Failure::ChangedReleaseId)
    {
        124
    } else {
        123
    };
    let immutable = !draft && case != Failure::MutablePublished;
    Ok(format!(
        "{{\"id\":{release_id},\"tag_name\":\"v0.1.1\",\"url\":\"https://api.github.com/repos/{REPOSITORY}/releases/{release_id}\",\"html_url\":\"https://github.com/{REPOSITORY}/releases/tag/v0.1.1\",\"draft\":{draft},\"prerelease\":false,\"immutable\":{immutable},\"assets\":[{}]}}\n",
        rows.join(","),
    ))
}

pub(super) fn install_mock_gh(root: &Path) -> Result<(), Box<dyn Error>> {
    super::fake_commands::install_mock_gh(root)
}

pub(super) fn path_with_mock_gh(root: &Path) -> Result<std::ffi::OsString, Box<dyn Error>> {
    let mut paths = vec![root.join("mock-bin")];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").ok_or("missing PATH")?,
    ));
    Ok(std::env::join_paths(paths)?)
}

fn preflight_mode(case: Failure) -> &'static str {
    match case {
        Failure::UnauthorizedTag => "401",
        Failure::ForbiddenTag => "403",
        Failure::TransientTag => "transient",
        _ => "404",
    }
}

fn expected_asset_args() -> String {
    format!("{}\n", manifest::release_asset_paths().replace(' ', "\n"))
}

fn assert_no_release_created(root: &Path) {
    assert!(!root.join("created-tag").exists());
}

pub(super) fn sha256(path: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("sha256sum").arg(path).output()?;
    if !output.status.success() {
        return Err(format!("sha256sum failed for {}", path.display()).into());
    }
    String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "sha256sum returned no digest".into())
}
