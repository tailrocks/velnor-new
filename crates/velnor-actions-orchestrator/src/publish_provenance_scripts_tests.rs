//! Behavioral cases: the Sigstore-bundle provenance scripts run for real
//! against fake `curl`, `gh` and Cosign executables on `PATH`.
//!
//! The fakes record their argv, so the cases prove what the generated job
//! asks of the real tools: the pinned Cosign is admitted by its exact byte
//! length and sha256, signing targets GitHub's Sigstore instance with no transparency
//! log, every bundle passes `gh attestation verify` with the strict identity
//! before a receipt exists, and the publish side refuses a bundle, receipt
//! or subject that does not match.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use velnor_actions_workflow_renderer::publish_scripts::provenance::{
    CosignPin, receipt_script, sign_script, stage_script, verify_script,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SUBJECT: &str = "libs-$ver-maven.zip";
const SUBJECT_FILE: &str = "libs-1.0.0-maven.zip";
const DIR: &str = "$RUNNER_TEMP/bundle";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn tools_present() -> bool {
    ["jq", "sha256sum"].iter().all(|tool| {
        Command::new(tool)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    })
}

/// Fake Cosign: records argv and writes a v0.3 bundle to `--bundle`.
const FAKE_COSIGN: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_LOG/cosign"
out=; while [ $# -gt 0 ]; do case "$1" in --bundle) out=$2; shift ;; esac; shift; done
printf '{"mediaType":"%s","verificationMaterial":{},"dsseEnvelope":{}}\n' "${FAKE_MEDIA:-application/vnd.dev.sigstore.bundle.v0.3+json}" > "$out"
"#;

/// Fake curl: records argv and copies the fake Cosign to `-o`.
const FAKE_CURL: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_LOG/curl"
out=; while [ $# -gt 0 ]; do case "$1" in -o) out=$2; shift ;; esac; shift; done
cp "$FAKE_COSIGN_SRC" "$out"
"#;

/// Fake gh: two trusted roots (public-good, GitHub) and a verifier whose
/// verdict is `$FAKE_VERIFY`; records every `attestation verify` argv.
const FAKE_GH: &str = r#"#!/bin/sh
case "$1 $2" in
  "attestation trusted-root")
    printf '%s\n' '{"certificateAuthorities":[{"uri":"https://fulcio.sigstore.dev"}],"tlogs":[{}]}'
    printf '%s\n' '{"certificateAuthorities":[{"uri":"fulcio.githubapp.com"},{"uri":"fulcio.githubapp.com"}],"tlogs":[]}' ;;
  "attestation verify")
    printf '%s\n' "$*" >> "$FAKE_LOG/gh"
    if [ "${FAKE_VERIFY_JSON+x}" = x ]; then
      printf '%s\n' "$FAKE_VERIFY_JSON"
    else
      printf '%s\n' '[{"verificationResult":{"signature":{"certificate":{"sourceRepositoryIdentifier":"123456","sourceRepositoryOwnerIdentifier":"654321"}}}}]'
    fi
    exit "${FAKE_VERIFY:-0}" ;;
  *) exit 64 ;;
esac
"#;

struct Fixture {
    _root: TempDir,
    bin: PathBuf,
    temp: PathBuf,
    log: PathBuf,
    cosign: PathBuf,
}

fn executable(path: &Path, body: &str) -> TestResult {
    fs::write(path, body)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let root = TempDir::new()?;
    let bin = root.path().join("bin");
    let temp = root.path().join("temp");
    let log = root.path().join("log");
    for dir in [&bin, &temp, &log] {
        fs::create_dir_all(dir)?;
    }
    let cosign = root.path().join("cosign-src");
    executable(&cosign, FAKE_COSIGN)?;
    executable(&bin.join("curl"), FAKE_CURL)?;
    executable(&bin.join("gh"), FAKE_GH)?;
    let bundle = temp.join("bundle");
    fs::create_dir_all(&bundle)?;
    fs::write(bundle.join(SUBJECT_FILE), "zip bytes")?;
    Ok(Fixture {
        _root: root,
        bin,
        temp,
        log,
        cosign,
    })
}

fn hex_sha256(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    use std::fmt::Write as _;
    let digest = Sha256::digest(fs::read(path)?);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        write!(hex, "{byte:02x}")?;
    }
    Ok(hex)
}

fn run(fx: &Fixture, script: &str, extra: &[(&str, &str)]) -> std::io::Result<Output> {
    let path = format!(
        "{}:{}",
        fx.bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new("sh");
    command
        .args(["-c", script])
        .env_clear()
        .env("PATH", path)
        .env("RUNNER_TEMP", &fx.temp)
        .env("FAKE_LOG", &fx.log)
        .env("FAKE_COSIGN_SRC", &fx.cosign)
        .env("GITHUB_REF_NAME", "libs-v1.0.0")
        .env("GITHUB_REF", "refs/tags/libs-v1.0.0")
        .env("GITHUB_REPOSITORY", "acme/widgets")
        .env("GITHUB_REPOSITORY_ID", "123456")
        .env("GITHUB_REPOSITORY_OWNER_ID", "654321")
        .env("GITHUB_SHA", SHA)
        .env("GITHUB_WORKFLOW_SHA", SHA)
        .env(
            "GITHUB_WORKFLOW_REF",
            "acme/widgets/.github/workflows/publish.yml@refs/tags/libs-v1.0.0",
        )
        .env("GITHUB_RUN_ID", "42")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_EVENT_NAME", "push");
    for (key, value) in extra {
        command.env(key, value);
    }
    command.output()
}

fn sign(
    fx: &Fixture,
    sha: &str,
    extra: &[(&str, &str)],
) -> Result<Output, Box<dyn std::error::Error>> {
    let pin = CosignPin {
        version: "3.1.4",
        sha256: sha,
        size: usize::try_from(fs::metadata(&fx.cosign)?.len())?,
    };
    let script = sign_script("gh() { command gh \"$@\"; }", DIR, SUBJECT, pin)?;
    Ok(run(fx, &script, extra)?)
}

fn log(fx: &Fixture, tool: &str) -> String {
    fs::read_to_string(fx.log.join(tool)).unwrap_or_default()
}

#[test]
fn signing_targets_github_sigstore_verifies_the_bundle_and_writes_the_receipt() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let fx = fixture()?;
    let pin = hex_sha256(&fx.cosign)?;
    let output = sign(&fx, &pin, &[])?;
    assert!(output.status.success(), "{output:?}");

    let curl = log(&fx, "curl");
    let cosign_size = fs::metadata(&fx.cosign)?.len().to_string();
    assert!(
        curl.contains(
            "https://github.com/sigstore/cosign/releases/download/v3.1.4/cosign-linux-amd64"
        ) && curl.contains("--proto =https")
            && curl.contains(&format!("--max-filesize {cosign_size}")),
        "{curl}"
    );
    let cosign = log(&fx, "cosign");
    assert!(
        cosign.starts_with("attest-blob --yes --signing-config ")
            && cosign.contains("--type slsaprovenance1")
            && cosign.contains("--trusted-root "),
        "{cosign}"
    );
    let gh = log(&fx, "gh");
    for expected in [
        "attestation verify",
        "/bundle/libs-1.0.0-maven.zip --bundle ",
        "/provenance/libs-1.0.0-maven.zip.sigstore.json",
        "--repo acme/widgets",
        &format!("--source-digest {SHA}"),
        "--source-ref refs/tags/libs-v1.0.0",
        "--signer-workflow acme/widgets/.github/workflows/publish.yml",
        &format!("--signer-digest {SHA}"),
        "--predicate-type https://slsa.dev/provenance/v1",
        "--format json",
    ] {
        assert!(gh.contains(expected), "{expected} in {gh}");
    }

    let provenance = fx.temp.join("provenance");
    let mut names: Vec<_> = fs::read_dir(&provenance)?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    names.sort();
    assert_eq!(
        names,
        [
            "libs-1.0.0-maven.zip.provenance.json",
            "libs-1.0.0-maven.zip.sigstore.json"
        ]
    );
    assert!(!fx.temp.join("velnor-sign").exists(), "work dir removed");
    let receipt: serde_json::Value = serde_json::from_str(&fs::read_to_string(
        provenance.join("libs-1.0.0-maven.zip.provenance.json"),
    )?)?;
    assert_eq!(receipt["provenance"], "sigstore_bundle");
    assert_eq!(receipt["sigstore_instance"], "github");
    assert_eq!(receipt["subject"]["name"], SUBJECT_FILE);
    assert_eq!(
        receipt["subject"]["sha256"],
        hex_sha256(&fx.temp.join("bundle").join(SUBJECT_FILE))?
    );
    assert_eq!(
        receipt["bundle"]["sha256"],
        hex_sha256(&provenance.join("libs-1.0.0-maven.zip.sigstore.json"))?
    );
    assert_eq!(receipt["source"]["sha"], SHA);
    assert_eq!(receipt["source"]["ref"], "refs/tags/libs-v1.0.0");
    assert_eq!(receipt["source"]["repository_id"], "123456");
    assert_eq!(receipt["source"]["repository_owner_id"], "654321");
    assert_eq!(receipt["workflow"]["path"], ".github/workflows/publish.yml");
    assert_eq!(receipt["run"]["id"], "42");
    let verify = receipt["verify"].as_str().unwrap_or_default();
    assert!(
        verify.starts_with("gh attestation verify libs-1.0.0-maven.zip --bundle libs-1.0.0-maven.zip.sigstore.json --repo acme/widgets"),
        "{verify}"
    );
    Ok(())
}

#[test]
fn the_signing_config_names_only_github_sigstore_and_no_transparency_log() -> TestResult {
    let pin = CosignPin {
        version: "3.1.4",
        sha256: &"a".repeat(64),
        size: 1,
    };
    let script = sign_script("gh() { :; }", DIR, SUBJECT, pin)?;
    assert!(script.contains(r#""caUrls":[{"url":"https://fulcio.githubapp.com""#));
    assert!(
        script.contains(r#""tsaUrls":[{"url":"https://timestamp.githubapp.com/api/v1/timestamp""#)
    );
    assert!(script.contains(r#""rekorTlogUrls":[]"#));
    assert!(!script.contains("sigstore.dev"), "never public-good");
    assert!(
        !script.contains("/attestations"),
        "never the attestation store"
    );
    let bad = CosignPin {
        version: "3.1",
        sha256: "A",
        size: 0,
    };
    assert_eq!(
        sign_script("gh() { :; }", DIR, SUBJECT, bad),
        Err("invalid_cosign_pin")
    );
    Ok(())
}

#[test]
fn a_cosign_digest_mismatch_or_a_failed_verification_fails_before_any_receipt() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let fx = fixture()?;
    let wrong = sign(&fx, &"0".repeat(64), &[])?;
    assert!(!wrong.status.success(), "unpinned cosign admitted");
    assert!(log(&fx, "cosign").is_empty(), "cosign ran unverified");

    let fx = fixture()?;
    let pin = hex_sha256(&fx.cosign)?;
    let wrong_size = CosignPin {
        version: "3.1.4",
        sha256: &pin,
        size: usize::try_from(fs::metadata(&fx.cosign)?.len())? + 1,
    };
    let script = sign_script("gh() { command gh \"$@\"; }", DIR, SUBJECT, wrong_size)?;
    assert!(
        !run(&fx, &script, &[])?.status.success(),
        "wrong-length Cosign admitted"
    );
    assert!(
        !fx.temp
            .join("provenance/libs-1.0.0-maven.zip.provenance.json")
            .exists()
    );

    let fx = fixture()?;
    let pin = hex_sha256(&fx.cosign)?;
    let rejected = sign(&fx, &pin, &[("FAKE_VERIFY", "1")])?;
    assert!(!rejected.status.success());
    assert!(
        !fx.temp
            .join("provenance/libs-1.0.0-maven.zip.provenance.json")
            .exists(),
        "no receipt for an unverified bundle"
    );

    let fx = fixture()?;
    let pin = hex_sha256(&fx.cosign)?;
    let old_format = sign(
        &fx,
        &pin,
        &[(
            "FAKE_MEDIA",
            "application/vnd.dev.sigstore.bundle+json;version=0.1",
        )],
    )?;
    assert!(!old_format.status.success(), "only v0.3 bundles");
    Ok(())
}

#[test]
fn the_publish_side_reverifies_and_refuses_a_mismatched_subject_or_receipt() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let fx = fixture()?;
    let pin = hex_sha256(&fx.cosign)?;
    assert!(sign(&fx, &pin, &[])?.status.success());
    let gh = "gh() { command gh \"$@\"; }";
    let verify = verify_script(gh, DIR, SUBJECT);
    let receipt = receipt_script(DIR, SUBJECT);

    let ok = run(&fx, &verify, &[])?;
    assert!(ok.status.success(), "{ok:?}");
    assert!(run(&fx, &receipt, &[])?.status.success());
    let refused = run(&fx, &verify, &[("FAKE_VERIFY", "1")])?;
    assert!(
        !refused.status.success(),
        "gh attestation verify is binding"
    );

    let other_commit = run(&fx, &verify, &[("GITHUB_SHA", &"f".repeat(40))])?;
    assert!(
        !other_commit.status.success(),
        "receipt bound to the source commit"
    );

    let subject = fx.temp.join("bundle").join(SUBJECT_FILE);
    fs::write(&subject, "tampered")?;
    assert!(!run(&fx, &verify, &[])?.status.success());
    assert!(!run(&fx, &receipt, &[])?.status.success());
    fs::write(&subject, "zip bytes")?;

    fs::write(fx.temp.join("provenance/extra"), "x")?;
    assert!(
        !run(&fx, &verify, &[])?.status.success(),
        "exactly two files"
    );
    fs::remove_file(fx.temp.join("provenance/extra"))?;

    let staged = run(&fx, &stage_script(DIR, SUBJECT), &[])?;
    assert!(staged.status.success(), "{staged:?}");
    for suffix in [".sigstore.json", ".provenance.json"] {
        assert!(
            fx.temp
                .join(format!("bundle/{SUBJECT_FILE}{suffix}"))
                .exists()
        );
    }
    Ok(())
}

#[path = "publish_provenance_scripts_tests/identity.rs"]
mod identity;
#[path = "publish_provenance_scripts_tests/scripts.rs"]
mod scripts;
