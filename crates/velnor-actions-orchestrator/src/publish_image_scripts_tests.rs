//! Behavioral cases: the fixed image scripts run for real against OCI fixtures.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_contract::ImagePlatform;
use velnor_actions_workflow_renderer::image_publish_scripts::{
    assemble_script, owner_script, push_script,
};

use crate::generator::sha256_hex;

type Tamper = fn(&Path) -> TestResult;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const ID: &str = "demo";
const SPDX: &str = "https://spdx.dev/Document";
const INDEX: &str = "application/vnd.oci.image.index.v1+json";
const MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

fn tools_present() -> bool {
    ["jq", "tar", "sha256sum"]
        .iter()
        .all(|tool| Command::new(tool).arg("--version").output().is_ok())
}

fn sh(script: &str, dir: &Path, env: &[(&str, &str)]) -> std::io::Result<Output> {
    Command::new("sh")
        .args(["-c", script])
        .current_dir(dir)
        .env_clear()
        .env(
            "PATH",
            "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin:/sbin",
        )
        .envs(env.iter().copied())
        .output()
}

/// Write one blob into a layout and return its `sha256:` digest.
fn blob(layout: &Path, bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let hex = sha256_hex(bytes);
    let dir = layout.join("blobs/sha256");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(&hex), bytes)?;
    Ok(format!("sha256:{hex}"))
}

fn descriptor(media: &str, digest: &str, size: usize, extra: &str) -> String {
    format!(r#"{{"mediaType":"{media}","digest":"{digest}","size":{size}{extra}}}"#)
}

/// BuildKit-shaped per-platform layout: image, attestation manifest, SBOM.
fn layout(arch: &str, work: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let root = work.join(format!("src-{arch}"));
    fs::create_dir_all(&root)?;
    let config = blob(&root, format!("{{\"arch\":\"{arch}\"}}").as_bytes())?;
    let layer = blob(&root, format!("layer-{arch}").as_bytes())?;
    let image = format!(
        r#"{{"schemaVersion":2,"mediaType":"{MANIFEST}","config":{},"layers":[{}]}}"#,
        descriptor("application/vnd.oci.image.config.v1+json", &config, 12, ""),
        descriptor("application/vnd.oci.image.layer.v1.tar", &layer, 8, "")
    );
    let image_digest = blob(&root, image.as_bytes())?;
    let statement = format!(
        r#"{{"_type":"https://in-toto.io/Statement/v0.1","predicateType":"{SPDX}","predicate":{{"spdxVersion":"SPDX-2.3","SPDXID":"SPDXRef-DOCUMENT","name":"{arch}"}}}}"#
    );
    let statement_digest = blob(&root, statement.as_bytes())?;
    let attestation = format!(
        r#"{{"schemaVersion":2,"mediaType":"{MANIFEST}","config":{},"layers":[{}]}}"#,
        descriptor("application/vnd.oci.image.config.v1+json", &config, 12, ""),
        descriptor(
            "application/vnd.in-toto+json",
            &statement_digest,
            statement.len(),
            &format!(r#","annotations":{{"in-toto.io/predicate-type":"{SPDX}"}}"#)
        )
    );
    let attestation_digest = blob(&root, attestation.as_bytes())?;
    let top = format!(
        r#"{{"schemaVersion":2,"mediaType":"{INDEX}","manifests":[{},{}]}}"#,
        descriptor(
            MANIFEST,
            &image_digest,
            image.len(),
            &format!(r#","platform":{{"architecture":"{arch}","os":"linux"}}"#)
        ),
        descriptor(
            MANIFEST,
            &attestation_digest,
            attestation.len(),
            &format!(
                r#","annotations":{{"vnd.docker.reference.digest":"{image_digest}","vnd.docker.reference.type":"attestation-manifest"}},"platform":{{"architecture":"unknown","os":"unknown"}}"#
            )
        )
    );
    let top_digest = blob(&root, top.as_bytes())?;
    fs::write(
        root.join("oci-layout"),
        "{\"imageLayoutVersion\":\"1.0.0\"}\n",
    )?;
    fs::write(
        root.join("index.json"),
        format!(
            r#"{{"schemaVersion":2,"mediaType":"{INDEX}","manifests":[{}]}}"#,
            descriptor(INDEX, &top_digest, top.len(), "")
        ),
    )?;
    Ok(top_digest)
}

/// Stage the artifact a build job uploads for one platform.
fn stage(arch: &str, work: &Path, runner_temp: &Path) -> TestResult {
    let top = layout(arch, work)?;
    let artifact = runner_temp.join(format!("velnor/image/{ID}/in/{arch}"));
    fs::create_dir_all(&artifact)?;
    let root = work.join(format!("src-{arch}"));
    let tar = Command::new("tar")
        .args(["-cf"])
        .arg(artifact.join("image.tar"))
        .arg("-C")
        .arg(&root)
        .arg(".")
        .output()?;
    assert!(tar.status.success());
    let sum = Command::new("sh")
        .args(["-c", "sha256sum image.tar > image.tar.sha256"])
        .env("PATH", "/usr/bin:/bin:/sbin")
        .current_dir(&artifact)
        .output()?;
    assert!(sum.status.success());
    fs::write(
        artifact.join("metadata.json"),
        format!(r#"{{"containerimage.digest":"{top}"}}"#),
    )?;
    Ok(())
}

struct Fixture {
    _work: TempDir,
    temp: TempDir,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let work = TempDir::new()?;
        let temp = TempDir::new()?;
        for arch in ["amd64", "arm64"] {
            stage(arch, work.path(), temp.path())?;
        }
        Ok(Self { _work: work, temp })
    }

    fn assemble(&self) -> Result<(Output, String), Box<dyn std::error::Error>> {
        let out = self.temp.path().join("github_output");
        fs::write(&out, "")?;
        let platforms = [ImagePlatform::LinuxAmd64, ImagePlatform::LinuxArm64];
        let output = sh(
            &assemble_script(ID, &platforms),
            self.temp.path(),
            &[
                ("RUNNER_TEMP", self.temp.path().to_str().ok_or("path")?),
                ("GITHUB_OUTPUT", out.to_str().ok_or("path")?),
            ],
        )?;
        Ok((output, fs::read_to_string(out)?))
    }

    fn artifact(&self, arch: &str) -> std::path::PathBuf {
        self.temp
            .path()
            .join(format!("velnor/image/{ID}/in/{arch}"))
    }
}

#[test]
fn assembled_index_keeps_every_platform_and_attestation_descriptor() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let (output, written) = fixture.assemble()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let merged = fixture
        .temp
        .path()
        .join(format!("velnor/image/{ID}/merged"));
    let index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(merged.join("index.json"))?)?;
    let digest = index["manifests"][0]["digest"].as_str().ok_or("digest")?;
    assert!(written.contains(&format!("digest={digest}\n")), "{written}");
    let hex = digest.trim_start_matches("sha256:");
    let bytes = fs::read(merged.join("blobs/sha256").join(hex))?;
    assert_eq!(sha256_hex(&bytes), hex, "digest names the index bytes");
    let top: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(top["manifests"].as_array().ok_or("manifests")?.len(), 4);
    for arch in ["amd64", "arm64"] {
        assert!(
            written.contains(&format!("digest_{arch}=sha256:")),
            "{written}"
        );
        let sbom: serde_json::Value = serde_json::from_str(&fs::read_to_string(
            fixture
                .temp
                .path()
                .join(format!("velnor/image/{ID}/sbom/{arch}.spdx.json")),
        )?)?;
        assert_eq!(sbom["name"], arch);
    }
    Ok(())
}

#[test]
fn tampered_or_unexpected_artifacts_fail_closed() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let tamper: [(&str, Tamper); 4] = [
        ("tar bytes", |dir| {
            let mut bytes = fs::read(dir.join("image.tar"))?;
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            fs::write(dir.join("image.tar"), bytes)?;
            Ok(())
        }),
        ("extra file", |dir| {
            fs::write(dir.join("extra"), "x")?;
            Ok(())
        }),
        ("metadata digest", |dir| {
            fs::write(
                dir.join("metadata.json"),
                format!(r#"{{"containerimage.digest":"sha256:{}"}}"#, "0".repeat(64)),
            )?;
            Ok(())
        }),
        ("missing sidecar", |dir| {
            fs::remove_file(dir.join("image.tar.sha256"))?;
            Ok(())
        }),
    ];
    for (name, change) in tamper {
        let fixture = Fixture::new()?;
        change(&fixture.artifact("arm64"))?;
        let (output, written) = fixture.assemble()?;
        assert!(!output.status.success(), "{name} must fail");
        assert!(!written.contains("digest=sha256:"), "{name}: {written}");
    }
    Ok(())
}

#[test]
fn archives_with_links_or_parent_paths_are_rejected() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let artifact = fixture.artifact("amd64");
    let evil = TempDir::new()?;
    fs::write(evil.path().join("index.json"), "{}")?;
    std::os::unix::fs::symlink("/etc/passwd", evil.path().join("link"))?;
    let tar = Command::new("tar")
        .arg("-cf")
        .arg(artifact.join("image.tar"))
        .arg("-C")
        .arg(evil.path())
        .arg(".")
        .output()?;
    assert!(tar.status.success());
    let sum = Command::new("sh")
        .args(["-c", "sha256sum image.tar > image.tar.sha256"])
        .env("PATH", "/usr/bin:/bin:/sbin")
        .current_dir(&artifact)
        .output()?;
    assert!(sum.status.success());
    let (output, _) = fixture.assemble()?;
    assert!(!output.status.success(), "a symlink member is refused");
    Ok(())
}

#[test]
fn owner_check_compares_the_lowercased_repository_owner() -> TestResult {
    let dir = TempDir::new()?;
    let run = |owner: &str| -> Result<bool, Box<dyn std::error::Error>> {
        Ok(sh(
            &owner_script("acme"),
            dir.path(),
            &[("GITHUB_REPOSITORY_OWNER", owner)],
        )?
        .status
        .success())
    };
    assert!(run("Acme")?);
    assert!(run("acme")?);
    assert!(!run("acme2")?);
    assert!(!run("evil")?);
    Ok(())
}

#[test]
fn push_is_immutable_per_commit_and_verifies_the_registry_digest() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let temp = TempDir::new()?;
    let merged = temp.path().join(format!("velnor/image/{ID}/merged"));
    fs::create_dir_all(&merged)?;
    let digest = format!("sha256:{}", "a".repeat(64));
    fs::write(
        merged.join("index.json"),
        format!(r#"{{"manifests":[{{"digest":"{digest}"}}]}}"#),
    )?;
    let fake = temp.path().join("crane");
    let log = temp.path().join("log");
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{log}'\ncase \"$1\" in digest) [ -n \"$HAVE\" ] || exit 1; echo \"$HAVE\" ;; push) : ;; tag) : ;; esac\n",
            log = log.display()
        ),
    )?;
    sh(&format!("chmod +x '{}'", fake.display()), temp.path(), &[])?;
    let function = format!("crane() {{ '{}' \"$@\"; }}", fake.display());
    let script = push_script(&function, ID, "ghcr.io/acme/demo", Some("main"));
    let base = [
        ("RUNNER_TEMP", temp.path().to_str().ok_or("path")?),
        ("GITHUB_SHA", "1111111111111111111111111111111111111111"),
    ];
    let run = |have: &str| -> Result<bool, Box<dyn std::error::Error>> {
        let mut env = base.to_vec();
        env.push(("HAVE", have));
        Ok(sh(&script, temp.path(), &env)?.status.success())
    };
    assert!(run(&digest)?, "pushed digest equals the assembled digest");
    let other = format!("sha256:{}", "b".repeat(64));
    assert!(!run(&other)?, "an existing tag with another digest fails");
    let text = fs::read_to_string(&log)?;
    assert!(text.contains("tag ghcr.io/acme/demo@sha256:"), "{text}");
    assert!(text.contains("push "), "{text}");
    Ok(())
}
