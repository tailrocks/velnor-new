use super::{assets, manifest};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const REPOSITORY: &str = "tailrocks/velnor-new";
const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
#[path = "schema2_generator_release_test_pins.rs"]
mod pins;
use pins::{PINNED_MISE_ARGUMENTS, test_pins};

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
    WrongSidecarName,
    MissingBinary,
    StaleManifest,
    UnauthorizedTag,
    ForbiddenTag,
    TransientTag,
    WrongPublishedDigest,
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
        Failure::WrongPublishedDigest,
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
    let release_json = release_json(&scratch.0, case)?;
    fs::write(scratch.0.join("release.json"), release_json)?;
    install_mock_gh(&scratch.0)?;
    let script = scratch.0.join("publish.sh");
    fs::write(&script, manifest::publish_script(&test_pins()))?;
    let gh_function = super::workflow_steps::gh_function(&test_pins().gh_argv)?;
    let output = run_publish_command(&scratch.0, case, &script, &gh_function)?;
    cli_tests::assert_pinned_publish_calls(&scratch.0, case)?;
    assert_publish_result(&scratch.0, case, &output)
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
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GH_PREFLIGHT", preflight_mode(case))
        .env("GH_RELEASE_JSON", root.join("release.json"))
        .env("GH_CREATE_TAG", root.join("created-tag"))
        .env("GH_ASSET_ARGS", root.join("asset-args"))
        .env("GH_CALLS", root.join("gh-calls"))
        .env("MISE_CALLS", root.join("mise-calls"))
        .env("MOCK_GH", root.join("mock-bin/gh"));
    cli_tests::isolate_gh_environment(&mut command, root)?;
    Ok(command.output()?)
}

fn assert_publish_result(
    root: &Path,
    case: Failure,
    output: &std::process::Output,
) -> Result<(), Box<dyn Error>> {
    let should_succeed = case == Failure::None;
    assert_eq!(
        output.status.success(),
        should_succeed,
        "case {case:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let create_is_expected = matches!(case, Failure::None | Failure::WrongPublishedDigest);
    if create_is_expected {
        assert_eq!(fs::read_to_string(root.join("created-tag"))?, "v0.1.1\n");
        assert_eq!(
            fs::read_to_string(root.join("asset-args"))?,
            expected_asset_args()
        );
    } else {
        assert_no_release_created(root);
    }
    Ok(())
}

fn copy_release_helpers(root: &Path) -> Result<(), Box<dyn Error>> {
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

fn write_candidate_records(root: &Path, case: Failure) -> Result<(), Box<dyn Error>> {
    let products = [assets::LINUX, assets::MACOS_ARM64];
    for (index, product) in products.into_iter().enumerate() {
        let directory = root.join(product.directory);
        fs::create_dir_all(&directory)?;
        let binary = directory.join(product.binary);
        if !(case == Failure::MissingBinary && index == 1) {
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

fn write_attestation_files(root: &Path) -> Result<(), Box<dyn Error>> {
    for path in manifest::release_asset_paths().split_whitespace() {
        if path.starts_with("release-attestations/") {
            let target = root.join(path);
            fs::create_dir_all(target.parent().ok_or("missing attestation parent")?)?;
            fs::write(target, b"signed bundle fixture\n")?;
        }
    }
    Ok(())
}

fn release_json(root: &Path, case: Failure) -> Result<String, Box<dyn Error>> {
    let mut rows = Vec::new();
    for (index, path) in manifest::release_asset_paths()
        .split_whitespace()
        .enumerate()
    {
        let asset = root.join(path);
        if !asset.exists() {
            continue;
        }
        let mut digest = sha256(&asset)?;
        if case == Failure::WrongPublishedDigest && index == 0 {
            digest = "0".repeat(64);
        }
        let name = Path::new(path)
            .file_name()
            .ok_or("release asset has no basename")?
            .to_str()
            .ok_or("release asset basename is not UTF-8")?;
        rows.push(format!(
            "{{\"name\":\"{name}\",\"digest\":\"sha256:{digest}\"}}"
        ));
    }
    Ok(format!(
        "{{\"tag_name\":\"v0.1.1\",\"draft\":false,\"prerelease\":false,\"immutable\":true,\"assets\":[{}]}}\n",
        rows.join(",")
    ))
}

fn install_mock_gh(root: &Path) -> Result<(), Box<dyn Error>> {
    let bin = root.join("mock-bin");
    fs::create_dir_all(&bin)?;
    let mock = bin.join("gh");
    fs::write(
        &mock,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$GH_CALLS"
if [ "$1" = attestation ] && [ "$2" = verify ]; then exit 0; fi
if [ "$1" = api ]; then
  shift
  if [ "$1" = --paginate ] && [ "$2" = --slurp ]; then
    case "$3" in
      *actions/workflows/ci.yml/runs*)
        printf '[{"workflow_runs":[{"id":91,"run_number":9,"run_attempt":1,"path":".github/workflows/ci.yml","head_sha":"%s","head_branch":"main","head_repository":{"full_name":"tailrocks/velnor-new"},"event":"push","status":"completed","conclusion":"success"}]}]\n' "$GITHUB_SHA" ;;
      *actions/runs/91/attempts/1/jobs*)
        printf '[{"jobs":[{"name":"Required","head_sha":"%s","head_branch":"main","status":"completed","conclusion":"success"}]}]\n' "$GITHUB_SHA" ;;
      *) exit 43 ;;
    esac
    exit 0
  fi
  if [ "$1" = --include ]; then
    case "$GH_PREFLIGHT" in
      404) printf 'HTTP/2 404\r\n\r\n{"message":"Not Found","status":"404"}\n'; exit 1 ;;
      401) printf 'HTTP/2 401\r\n\r\n{"message":"Bad credentials","status":"401"}\n'; exit 1 ;;
      403) printf 'HTTP/2 403\r\n\r\n{"message":"Forbidden","status":"403"}\n'; exit 1 ;;
      transient) exit 22 ;;
    esac
  fi
  case "$1" in
    repos/tailrocks/velnor-new/commits/main)
      if [ "$2" = --jq ] && [ "$3" = .sha ]; then printf '%s\n' "$GITHUB_SHA"; else printf '{"sha":"%s"}\n' "$GITHUB_SHA"; fi ;;
    repos/tailrocks/velnor-new/git/ref/tags/v0.1.1)
      printf '{"object":{"type":"commit","sha":"%s"}}\n' "$GITHUB_SHA" ;;
    repos/tailrocks/velnor-new/releases/tags/v0.1.1)
      cat "$GH_RELEASE_JSON" ;;
    *) exit 44 ;;
  esac
  exit 0
fi
if [ "$1" = attestation ] && [ "$2" = verify ]; then exit 0; fi
if [ "$1" = release ] && [ "$2" = create ]; then
  test "$3" = v0.1.1
  printf '%s\n' "$3" > "$GH_CREATE_TAG"
  shift 3
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -R) test "$2" = tailrocks/velnor-new; shift 2 ;;
      --target) test "$2" = "$GITHUB_SHA"; shift 2 ;;
      --title) test "$2" = 'velnor-actions v0.1.1'; shift 2 ;;
      --latest=false) shift ;;
      --notes) shift 2; printf '%s\n' "$@" > "$GH_ASSET_ARGS"; exit 0 ;;
      *) exit 45 ;;
    esac
  done
fi
exit 46
"#,
    )?;
    let mut permissions = fs::metadata(&mock)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(mock, permissions)?;
    let git = bin.join("git");
    fs::write(
        &git,
        "#!/bin/sh\nset -eu\nif [ \"$1\" = rev-parse ] && [ \"$2\" = HEAD ]; then printf '%s\\n' \"$GITHUB_SHA\"; exit 0; fi\nexec /usr/bin/git \"$@\"\n",
    )?;
    let mut permissions = fs::metadata(&git)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(git, permissions)?;
    cli_tests::install_mock_mise(root)?;
    Ok(())
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

fn sha256(path: &Path) -> Result<String, Box<dyn Error>> {
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

#[path = "schema2_generator_release_cli_tests.rs"]
mod cli_tests;
