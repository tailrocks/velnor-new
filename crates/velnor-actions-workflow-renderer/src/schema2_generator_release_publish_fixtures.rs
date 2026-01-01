//! Publish-script fixture writers shared by release integration tests.

use super::super::assets;
use super::super::required_tool_path::with_required_tool_path;
use super::{Failure, REPOSITORY, SOURCE_SHA, manifest};
use crate::schema2::product_release_test_pins::test_pins;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn copy_release_helpers(root: &Path) -> Result<(), Box<dyn Error>> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
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

pub(super) fn write_candidate_records(root: &Path, case: Failure) -> Result<(), Box<dyn Error>> {
    let pins = test_pins();
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
                "{{\"schema\":1,\"version\":\"0.1.7\",\"repository\":\"{REPOSITORY}\",\"commit\":\"{SOURCE_SHA}\",\"target\":\"{}\",\"asset\":\"{}\",\"sha256\":\"{digest}\",\"toolchain\":{{\"rust\":\"{}\",\"mr-boxington\":\"{}\"}}}}\n",
                product.target.triple(),
                product.binary,
                pins.rust_version,
                pins.mr_boxington_version
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

pub(super) fn release_json(
    root: &Path,
    case: Failure,
    draft: bool,
) -> Result<String, Box<dyn Error>> {
    let rows = release_asset_rows(root, case, draft)?;
    let release_id = if (draft && case == Failure::WrongDraftReleaseId)
        || (!draft && case == Failure::ChangedReleaseId)
    {
        124
    } else {
        123
    };
    let immutable = !draft && case != Failure::MutablePublished;
    let html_url = if (draft && case == Failure::UntaggedDraftUrls)
        || (!draft && case == Failure::WrongPublishedUrl)
    {
        "https://github.com/tailrocks/velnor-new/releases/tag/untagged-6899c9b4aa4e941dadba"
    } else {
        "https://github.com/tailrocks/velnor-new/releases/tag/v0.1.7"
    };
    let tag_name = if draft && case == Failure::WrongDraftTag {
        "v0.1.4"
    } else {
        "v0.1.7"
    };
    let source = if draft && case == Failure::WrongDraftSource {
        "1111111111111111111111111111111111111111"
    } else {
        SOURCE_SHA
    };
    let api_url = if draft && case == Failure::WrongDraftRepository {
        format!("https://api.github.com/repos/untrusted/velnor-new/releases/{release_id}")
    } else if draft && case == Failure::WrongDraftApiPath {
        format!("https://api.github.com/repos/{REPOSITORY}/releases/tags/v0.1.7")
    } else {
        format!("https://api.github.com/repos/{REPOSITORY}/releases/{release_id}")
    };
    Ok(format!(
        "{{\"id\":{release_id},\"tag_name\":\"{tag_name}\",\"target_commitish\":\"{source}\",\"url\":\"{api_url}\",\"html_url\":\"{html_url}\",\"draft\":{draft},\"prerelease\":false,\"immutable\":{immutable},\"assets\":[{}]}}\n",
        rows.join(","),
    ))
}

fn release_asset_rows(
    root: &Path,
    case: Failure,
    draft: bool,
) -> Result<Vec<String>, Box<dyn Error>> {
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
        let expected_name = Path::new(path)
            .file_name()
            .ok_or("release asset has no basename")?
            .to_str()
            .ok_or("release asset basename is not UTF-8")?;
        let name = if draft && case == Failure::WrongDraftAssetName && index == 0 {
            "unrelated-asset"
        } else {
            expected_name
        };
        let size = fs::metadata(&asset)?.len()
            + u64::from(draft && case == Failure::WrongDraftSize && index == 0);
        let url = if draft {
            format!(
                "https://github.com/{REPOSITORY}/releases/download/untagged-6899c9b4aa4e941dadba/{expected_name}"
            )
        } else if case == Failure::WrongPublishedAssetUrl && index == 0 {
            format!("https://github.com/untrusted/releases/download/v0.1.7/{name}")
        } else {
            format!("https://github.com/{REPOSITORY}/releases/download/v0.1.7/{name}")
        };
        let state = if draft && case == Failure::WrongDraftAssetState && index == 0 {
            "starter"
        } else {
            "uploaded"
        };
        rows.push(format!(
            "{{\"name\":\"{name}\",\"state\":\"{state}\",\"browser_download_url\":\"{url}\",\"digest\":\"sha256:{digest}\",\"size\":{size}}}"
        ));
    }
    if draft && case == Failure::UnknownDraftAsset {
        rows.push(
            "{\"name\":\"unknown-asset\",\"state\":\"uploaded\",\"browser_download_url\":\"https://github.com/tailrocks/velnor-new/releases/download/untagged-6899c9b4aa4e941dadba/unknown-asset\",\"digest\":\"sha256:0000000000000000000000000000000000000000000000000000000000000000\",\"size\":0}".to_owned(),
        );
    }
    Ok(rows)
}

pub(super) fn assert_draft_metadata_is_only_url_mismatch(
    root: &Path,
) -> Result<(), Box<dyn Error>> {
    let script = manifest::publish_script(&super::test_pins())?;
    let start = script
        .find("verify_release_metadata() {")
        .ok_or("publisher metadata verifier is missing")?;
    let end = script[start..]
        .find("\n}\n")
        .map(|offset| start + offset + 2)
        .ok_or("publisher metadata verifier is unterminated")?;
    let verifier = &script[start..end];
    let replay = format!(
        "release_id='123'\ntag='v0.1.7'\n{verifier}\nverify_release_metadata draft-release.json true\n"
    );
    let output = Command::new("bash")
        .args(["-euo", "pipefail", "-c", &replay])
        .current_dir(root)
        .env("PATH", with_required_tool_path(&[])?)
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("GITHUB_SHA", SOURCE_SHA)
        .output()?;
    assert!(
        output.status.success(),
        "valid draft identity/source metadata failed independently: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let checks = [
        (".id == $id", true),
        (".tag_name == $tag", true),
        (".target_commitish == $source", true),
        (".url == $api_url", true),
        (".html_url == $html_url", false),
        (".draft == true", true),
        (".prerelease == false", true),
    ];
    for (predicate, expected) in checks {
        let output = Command::new("jq")
            .args([
                "-e",
                "--argjson",
                "id",
                "123",
                "--arg",
                "tag",
                "v0.1.7",
                "--arg",
                "source",
                SOURCE_SHA,
                "--arg",
                "api_url",
                "https://api.github.com/repos/tailrocks/velnor-new/releases/123",
                "--arg",
                "html_url",
                "https://github.com/tailrocks/velnor-new/releases/tag/v0.1.7",
                predicate,
                "draft-release.json",
            ])
            .current_dir(root)
            .env("PATH", with_required_tool_path(&[])?)
            .output()?;
        assert_eq!(
            output.status.success(),
            expected,
            "unexpected result for draft metadata predicate {predicate}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub(super) fn assert_draft_asset_validation_passes(root: &Path) -> Result<(), Box<dyn Error>> {
    let script = manifest::publish_script(&super::test_pins())?;
    let start = script
        .find("verify_release_assets() {")
        .ok_or("publisher asset verifier is missing")?;
    let end = script[start..]
        .find("\n}\n")
        .map(|offset| start + offset + 2)
        .ok_or("publisher asset verifier is unterminated")?;
    let verifier = &script[start..end];
    let paths = manifest::release_asset_paths()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for path in &paths {
        assert!(root.join(path).is_file(), "missing fixture asset {path}");
    }
    let names = paths
        .iter()
        .map(|path| {
            path.rsplit('/')
                .next()
                .map(str::to_owned)
                .ok_or("release asset has no basename")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let array = |values: &[String]| {
        values
            .iter()
            .map(|value| format!("  '{value}'"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let replay = format!(
        "tag='v0.1.7'\nrelease_asset_paths=(\n{}\n)\nrelease_asset_names=(\n{}\n)\nexpected_release_asset_names=\"$(printf '%s\\n' \"${{release_asset_names[@]}}\" | jq -R . | jq -s .)\"\n{verifier}\nverify_release_assets draft-release.json false\n",
        array(&paths),
        array(&names)
    );
    let output = Command::new("bash")
        .args(["-euo", "pipefail", "-c", &replay])
        .current_dir(root)
        .env("PATH", with_required_tool_path(&[])?)
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .output()?;
    assert!(
        output.status.success(),
        "exact draft asset inventory failed independently: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

pub(super) fn sha256(path: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("sha256sum")
        .arg(path)
        .env("PATH", with_required_tool_path(&[])?)
        .output()?;
    if !output.status.success() {
        return Err(format!("sha256sum failed for {}", path.display()).into());
    }
    String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "sha256sum returned no digest".into())
}
