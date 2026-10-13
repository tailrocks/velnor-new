use super::{RenderedCommands, scripts};
use crate::tool_seed_test_support::git_fixture;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use velnor_actions_contract::RustBinaryReleaseConfig;

const REPOSITORY: &str = "owner/demo";
const PACKAGE: &str = "demo-package";
const BINARY: &str = "demo-binary";
type TaggedVersions = Vec<(String, String)>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!(
            "velnor-binary-release-tags-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self(root))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[test]
fn resolver_selects_tags_by_semver_precedence_and_rejects_invalid_versions()
-> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let repository = scratch.0.join("repository");
    let (trusted_sha, tag_shas) = create_tagged_repository(
        &repository,
        &[
            "1.9.9",
            "1.10.0-rc.1",
            "1.10.0+build.1",
            "1.10.0",
            "01.11.0",
            "1.11.0-rc.01",
        ],
    )?;
    let stable_sha = tag_shas
        .iter()
        .find(|(version, _)| version == "1.10.0")
        .map(|(_, sha)| sha.clone())
        .ok_or("stable release fixture is missing")?;
    let bin = scratch.0.join("bin");
    fs::create_dir_all(&bin)?;
    install_mock_gh(&bin.join("gh"))?;
    let github_output = scratch.0.join("github-output");
    fs::write(&github_output, "")?;
    let script = scripts::verify_source(
        &RustBinaryReleaseConfig {
            enabled: true,
            manifest_path: "Cargo.toml".to_owned(),
            package: PACKAGE.to_owned(),
            binary: Some(BINARY.to_owned()),
            source_commit_env: None,
        },
        BINARY,
        &mock_commands(),
    );
    let output = run_resolver(
        &script,
        &repository,
        &scratch.0,
        &bin,
        &github_output,
        &trusted_sha,
    )?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outputs = fs::read_to_string(github_output)?;
    assert!(outputs.contains("should_release=true"), "{outputs}");
    assert!(
        outputs.contains(&format!("source_sha={stable_sha}")),
        "{outputs}"
    );
    assert!(
        outputs.contains(&format!("default_sha={trusted_sha}")),
        "{outputs}"
    );
    assert!(outputs.contains("version=1.10.0"), "{outputs}");
    assert!(outputs.contains("tag=demo-package-v1.10.0"), "{outputs}");
    Ok(())
}

#[test]
fn resolver_orders_numeric_prereleases_and_selects_rc_without_a_stable_tag()
-> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let repository = scratch.0.join("repository");
    let (trusted_sha, tag_shas) =
        create_tagged_repository(&repository, &["1.10.0-rc.2", "1.10.0-rc.10"])?;
    let rc10_sha = tag_shas
        .iter()
        .find(|(version, _)| version == "1.10.0-rc.10")
        .map(|(_, sha)| sha.clone())
        .ok_or("highest prerelease fixture is missing")?;
    let bin = scratch.0.join("bin");
    fs::create_dir_all(&bin)?;
    install_mock_gh(&bin.join("gh"))?;
    let github_output = scratch.0.join("github-output");
    fs::write(&github_output, "")?;
    let script = scripts::verify_source(
        &RustBinaryReleaseConfig {
            enabled: true,
            manifest_path: "Cargo.toml".to_owned(),
            package: PACKAGE.to_owned(),
            binary: Some(BINARY.to_owned()),
            source_commit_env: None,
        },
        BINARY,
        &mock_commands(),
    );
    let output = run_resolver(
        &script,
        &repository,
        &scratch.0,
        &bin,
        &github_output,
        &trusted_sha,
    )?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outputs = fs::read_to_string(github_output)?;
    assert!(outputs.contains("should_release=true"), "{outputs}");
    assert!(
        outputs.contains(&format!("source_sha={rc10_sha}")),
        "{outputs}"
    );
    assert!(outputs.contains("version=1.10.0-rc.10"), "{outputs}");
    assert!(
        outputs.contains("tag=demo-package-v1.10.0-rc.10"),
        "{outputs}"
    );
    Ok(())
}

#[test]
fn publisher_passes_prerelease_flag_only_for_semver_prereleases() -> Result<(), Box<dyn Error>> {
    for (version, expected_prerelease) in [
        ("1.10.0", false),
        ("1.10.0+build-1", false),
        ("1.10.0-rc.1", true),
        ("1.10.0-rc.1+build-1", true),
    ] {
        let scratch = Scratch::new()?;
        let assets = scratch.0.join("assets");
        fs::create_dir_all(&assets)?;
        let linux_name = format!("{BINARY}-{version}-x86_64-unknown-linux-gnu.tar.gz");
        let macos_name = format!("{BINARY}-{version}-aarch64-apple-darwin.tar.gz");
        fs::write(assets.join(&linux_name), b"linux artifact")?;
        fs::write(assets.join(&macos_name), b"macos artifact")?;
        let checksums = Command::new("sha256sum")
            .args([&linux_name, &macos_name])
            .current_dir(&assets)
            .output()?;
        if !checksums.status.success() {
            return Err(String::from_utf8_lossy(&checksums.stderr)
                .into_owned()
                .into());
        }
        fs::write(assets.join("SHA256SUMS"), checksums.stdout)?;

        let bin = scratch.0.join("bin");
        fs::create_dir_all(&bin)?;
        let captured_args = scratch.0.join("release-args");
        let source_sha = "0123456789abcdef0123456789abcdef01234567";
        let default_sha = "abcdef0123456789abcdef0123456789abcdef01";
        install_mock_publisher_gh(&bin.join("gh"))?;
        let script = scripts::publish(PACKAGE, BINARY, "gh");
        assert!(
            !script.contains("is_prerelease"),
            "publisher script must not define an is_prerelease variable"
        );
        let path = format!("{}:{}", bin.display(), std::env::var("PATH")?);
        let output = Command::new("bash")
            .args(["-c", &script])
            .current_dir(&scratch.0)
            .env("PATH", path)
            .env("GH_TOKEN", "write-token-for-test")
            .env("GITHUB_REPOSITORY", REPOSITORY)
            .env("SOURCE_SHA", source_sha)
            .env("DEFAULT_SHA", default_sha)
            .env("RELEASE_VERSION", version)
            .env("RELEASE_TAG", format!("{PACKAGE}-v{version}"))
            .env("MOCK_REPOSITORY", REPOSITORY)
            .env("MOCK_SOURCE_SHA", source_sha)
            .env("MOCK_TAG", format!("{PACKAGE}-v{version}"))
            .env("MOCK_RELEASE_ARGS", &captured_args)
            .output()?;

        assert!(
            output.status.success(),
            "version {version}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let args = fs::read(captured_args)?
            .split(|byte| *byte == 0)
            .filter(|argument| !argument.is_empty())
            .map(|argument| String::from_utf8_lossy(argument).into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args.first().map(String::as_str), Some("release"));
        assert_eq!(args.get(1).map(String::as_str), Some("create"));
        assert_eq!(
            args.iter()
                .filter(|argument| argument.as_str() == "--prerelease")
                .count(),
            usize::from(expected_prerelease),
            "version {version}: {args:?}"
        );
    }
    Ok(())
}

fn create_tagged_repository(
    root: &Path,
    versions: &[&str],
) -> Result<(String, TaggedVersions), Box<dyn Error>> {
    fs::create_dir_all(root)?;
    run_git(root, &["init", "--quiet"])?;
    run_git(root, &["config", "user.name", "Binary Release Test"])?;
    run_git(
        root,
        &["config", "user.email", "binary-release@example.invalid"],
    )?;
    let mut tag_shas = Vec::with_capacity(versions.len());
    for version in versions {
        tag_shas.push(((*version).to_owned(), commit_tagged_version(root, version)?));
    }
    let trusted_sha = String::from_utf8(run_git(root, &["rev-parse", "HEAD"])?.stdout)?
        .trim()
        .to_owned();
    Ok((trusted_sha, tag_shas))
}

fn commit_tagged_version(root: &Path, version: &str) -> Result<String, Box<dyn Error>> {
    fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nname = \"{PACKAGE}\"\nversion = \"{version}\"\n"),
    )?;
    fs::write(root.join("source.txt"), format!("version {version}\n"))?;
    run_git(root, &["add", "Cargo.toml", "source.txt"])?;
    run_git(
        root,
        &["commit", "--quiet", "-m", &format!("version {version}")],
    )?;
    run_git(root, &["tag", &format!("{PACKAGE}-v{version}")])?;
    Ok(
        String::from_utf8(run_git(root, &["rev-parse", "HEAD"])?.stdout)?
            .trim()
            .to_owned(),
    )
}

fn run_git(root: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let output = git_fixture::command(root)?.args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output)
}

fn install_mock_gh(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
[ "$1" = api ] || exit 80
shift
if [ "${1-}" = --paginate ]; then shift; fi
endpoint="${1-}"
shift
case "$endpoint" in
  "repos/$MOCK_REPOSITORY") printf '{"default_branch":"main"}\n' ;;
  "repos/$MOCK_REPOSITORY/commits/main")
    if [ "${1-}" = --jq ] && [ "${2-}" = .sha ]; then
      printf '%s\n' "$MOCK_SHA"
    else
      printf '{"sha":"%s"}\n' "$MOCK_SHA"
    fi
    ;;
  "repos/$MOCK_REPOSITORY/releases?per_page=100") : ;;
  *) printf 'unexpected gh API endpoint: %s\n' "$endpoint" >&2; exit 81 ;;
esac
"#,
    )?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn install_mock_publisher_gh(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
if [ "$1" = api ]; then
  shift
  paginate=false; slurp=false; endpoint=
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --paginate) paginate=true; shift ;;
      --slurp) slurp=true; shift ;;
      *) endpoint="$1"; shift; break ;;
    esac
  done
  case "$endpoint" in
    "repos/$MOCK_REPOSITORY/git/ref/tags/$MOCK_TAG")
      printf '{"ref":"refs/tags/%s","object":{"type":"commit","sha":"%s"}}\n' "$MOCK_TAG" "$MOCK_SOURCE_SHA"
      ;;
    */compare/*)
      printf '{"status":"identical"}\n'
      ;;
    "repos/$MOCK_REPOSITORY/releases?per_page=100")
      [ "$paginate" = true ] && [ "$slurp" = true ] || exit 80
      printf '[[]]\n'
      ;;
    *) printf 'unexpected gh API endpoint: %s\n' "$endpoint" >&2; exit 81 ;;
  esac
elif [ "$1" = release ] && [ "${2-}" = create ]; then
  printf '%s\0' "$@" > "$MOCK_RELEASE_ARGS"
else
  printf 'unexpected gh command: %s\n' "$*" >&2
  exit 82
fi
"#,
    )?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn mock_commands() -> RenderedCommands {
    RenderedCommands {
        install_rust: String::new(),
        metadata: format!(
            "jq -nc --arg version \"$(sed -n 's/^version = \"\\([^\"]*\\)\"$/\\1/p' Cargo.toml | head -n 1)\" --arg package '{PACKAGE}' --arg binary '{BINARY}' '{{packages:[{{name:$package,version:$version,targets:[{{name:$binary,kind:[\"bin\"]}}]}}]}}'"
        ),
        rustc_version: String::new(),
        rust_version: String::new(),
        build_linux: String::new(),
        build_macos: String::new(),
        install_gh: String::new(),
        gh_prefix: "gh".to_owned(),
    }
}

fn run_resolver(
    script: &str,
    repository: &Path,
    runner_temp: &Path,
    bin: &Path,
    github_output: &Path,
    trusted_sha: &str,
) -> Result<Output, Box<dyn Error>> {
    let path = format!("{}:{}", bin.display(), std::env::var("PATH")?);
    Ok(Command::new("bash")
        .args(["-c", script])
        .current_dir(repository)
        .env("PATH", path)
        .env("GH_TOKEN", "read-only-test-token")
        .env("GITHUB_EVENT_NAME", "schedule")
        .env("GITHUB_SHA", trusted_sha)
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_NAME", "main")
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("MOCK_REPOSITORY", REPOSITORY)
        .env("MOCK_SHA", trusted_sha)
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_OUTPUT", github_output)
        .output()?)
}
