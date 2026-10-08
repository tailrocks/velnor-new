use super::manifest;
use crate::schema2::product_release_test_pins::test_pins;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const REPOSITORY: &str = "tailrocks/velnor-new";
const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const PINNED_MISE_ARGUMENTS: &str = "--no-config --no-env --no-hooks exec gh@2.102.0 --";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
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
enum Failure {
    None,
    DuplicateRequired,
    WrongSidecarName,
    MissingBinary,
    StaleManifest,
    UnauthorizedTag,
    ForbiddenTag,
    TransientTag,
    TagMovedBeforePublish,
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
        Failure::DuplicateRequired,
        Failure::WrongSidecarName,
        Failure::MissingBinary,
        Failure::StaleManifest,
        Failure::UnauthorizedTag,
        Failure::ForbiddenTag,
        Failure::TransientTag,
        Failure::TagMovedBeforePublish,
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
    fs::write(&script, manifest::publish_script(&test_pins())?)?;
    let gh_function = super::workflow_steps::gh_function(&test_pins().gh_argv)?;
    let output = run_publish_command(&scratch.0, case, &script, &gh_function)?;
    assert_publish_result(&scratch.0, case, &output)?;
    cli_tests::assert_pinned_publish_calls(&scratch.0, case)
}

fn create_candidate_manifest(root: &Path, case: Failure) -> Result<(), Box<dyn Error>> {
    let manifest_status = Command::new("bash")
        .args([
            "scripts/generator-release/create-release-manifest.sh",
            "0.1.3",
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
        .env(
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main",
        )
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_OUTPUT", root.join("eligibility-output"))
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
        .env("GH_TAG_SOURCE", root.join("tag-source"))
        .env("GH_TAG_READS", root.join("tag-reads"))
        .env("GH_MOVED_SHA", "1111111111111111111111111111111111111111")
        .env("MOCK_GH", root.join("mock-bin/gh"));
    cli_tests::isolate_gh_environment(&mut command, root)?;
    command.env("GH_TOKEN", "fixture-token");
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
            | Failure::TagMovedBeforePublish
    );
    if create_is_expected {
        assert_eq!(fs::read_to_string(root.join("created-tag"))?, "v0.1.3\n");
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
            publisher_receipt_tests::assert_success_receipt(root)?;
        } else {
            assert!(!root.join("release-accepted").exists());
        }
    } else {
        assert_no_release_created(root);
    }
    if case == Failure::TagMovedBeforePublish {
        assert_eq!(
            fs::read_to_string(root.join("tag-source"))?,
            "1111111111111111111111111111111111111111\n"
        );
        assert_eq!(fs::read_to_string(root.join("tag-reads"))?, "2\n");
        assert!(
            !root.join("patch-log").exists(),
            "publisher must not make the release public after the tag moves"
        );
        assert!(!root.join("release-accepted").exists());
    }
    Ok(())
}

fn install_mock_gh(root: &Path) -> Result<(), Box<dyn Error>> {
    fake_commands::install_mock_gh(root)
}

fn path_with_mock_gh(root: &Path) -> Result<std::ffi::OsString, Box<dyn Error>> {
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

#[path = "schema2_generator_release_cli_tests.rs"]
mod cli_tests;
#[path = "schema2_generator_release_publish_fixtures.rs"]
mod publish_fixtures;
use publish_fixtures::{
    copy_release_helpers, release_json, sha256, write_attestation_files, write_candidate_records,
};

#[path = "schema2_generator_release_fake_commands.rs"]
mod fake_commands;

#[path = "schema2_generator_release_manifest_publish_tests.rs"]
mod publisher_receipt_tests;
