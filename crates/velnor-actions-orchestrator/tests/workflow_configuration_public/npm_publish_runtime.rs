// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use base64::Engine as _;
use serde_json::Value;
use sha1_checked::Sha1;
use sha2::{Digest as _, Sha512};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::publish_render::PUBLISH_WORKFLOW_PATH;

use super::{make_bun_repo, write_bun_fixture};

const MEMBER: &str = "@fixture/web";
const VERSION: &str = "1.2.3";
const PUBLISH_TARBALL_NAME: &str = "@fixture/web-1.2.3.tgz";
const PUBLISH_TARBALL_URI: &str = "http://npm.pkg.github.com/@fixture/web/-/@fixture/web-1.2.3.tgz";

#[test]
fn generated_package_publisher_sends_verified_archive_metadata() -> Result<(), Box<dyn Error>> {
    let case = PublisherCase::new()?;
    let archive = case.write_archive()?;
    let script = generated_publish_script()?;
    let output = case.run(&script, None)?;

    assert!(output.status.success(), "publisher failed: {output:?}");
    assert!(case.curl_marker.exists(), "publisher did not call curl");
    assert_eq!(
        fs::read_to_string(&case.curl_args)?,
        "--silent\n--show-error\n--fail-with-body\n--config\n-\n--request\nPUT\n--header\nContent-Type: application/json\n--data-binary\n@document.json\nhttps://npm.pkg.github.com/@fixture%2Fweb\n"
    );
    assert_eq!(
        fs::read_to_string(&case.curl_config)?,
        "header = \"Authorization: Bearer fixture-token\"\n"
    );

    let request: Value = serde_json::from_slice(&fs::read(&case.curl_body)?)?;
    assert_eq!(request["_id"], MEMBER);
    assert_eq!(request["name"], MEMBER);
    assert_eq!(request["dist-tags"]["latest"], VERSION);
    assert_eq!(request["versions"][VERSION]["name"], MEMBER);
    assert_eq!(request["versions"][VERSION]["version"], VERSION);
    assert_eq!(request["versions"][VERSION]["customMetadata"], "preserved");

    let archive_bytes = fs::read(archive)?;
    let sha1 = Sha1::try_digest(&archive_bytes);
    assert!(!sha1.has_collision());
    let sha1_bytes: &[u8] = sha1.hash().as_ref();
    let mut sha1 = String::with_capacity(40);
    for byte in sha1_bytes {
        write!(&mut sha1, "{byte:02x}")?;
    }
    let sha512 = base64::engine::general_purpose::STANDARD.encode(Sha512::digest(&archive_bytes));
    let version = &request["versions"][VERSION];
    assert_eq!(version["dist"]["shasum"], sha1);
    assert_eq!(version["dist"]["integrity"], format!("sha512-{sha512}"));
    assert_eq!(version["dist"]["tarball"], PUBLISH_TARBALL_URI);

    let attachment = &request["_attachments"][PUBLISH_TARBALL_NAME];
    assert_eq!(attachment["content_type"], "application/octet-stream");
    assert_eq!(attachment["length"], archive_bytes.len());
    assert_eq!(
        base64::engine::general_purpose::STANDARD.decode(
            attachment["data"]
                .as_str()
                .ok_or("attachment data missing")?
        )?,
        archive_bytes
    );
    Ok(())
}

#[test]
fn generated_package_publisher_rejects_missing_and_corrupt_archives_before_curl()
-> Result<(), Box<dyn Error>> {
    let script = generated_publish_script()?;
    for archive_state in [ArchiveState::Missing, ArchiveState::Corrupt] {
        let case = PublisherCase::new()?;
        match archive_state {
            ArchiveState::Missing => {}
            ArchiveState::Corrupt => {
                fs::write(case.archive_path(), b"not a tar archive")?;
            }
        }

        let output = case.run(&script, None)?;
        assert!(
            !output.status.success(),
            "publisher accepted {archive_state:?} input: {output:?}"
        );
        assert!(
            !case.curl_marker.exists(),
            "publisher called curl for {archive_state:?} input"
        );
    }
    Ok(())
}

#[test]
fn generated_package_publisher_stops_when_openssl_digest_fails() -> Result<(), Box<dyn Error>> {
    let case = PublisherCase::new()?;
    case.write_archive()?;
    let script = generated_publish_script()?;
    let openssl = case.install_digest_failure_openssl()?;
    let output = case.run(&script, Some(&openssl))?;

    assert!(!output.status.success(), "publisher ignored a digest error");
    assert!(
        case.digest_marker.exists(),
        "digest failure fixture did not run"
    );
    assert!(
        !case.encoder_marker.exists(),
        "publisher continued to Base64 after a digest error"
    );
    assert!(
        !case.curl_marker.exists(),
        "publisher called curl after a digest error"
    );
    Ok(())
}

#[derive(Debug)]
enum ArchiveState {
    Missing,
    Corrupt,
}

struct PublisherCase {
    root: tempfile::TempDir,
    runner_temp: PathBuf,
    curl_marker: PathBuf,
    curl_args: PathBuf,
    curl_config: PathBuf,
    curl_body: PathBuf,
    digest_marker: PathBuf,
    encoder_marker: PathBuf,
}

impl PublisherCase {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let runner_temp = root.path().join("runner-temp");
        let npm_dir = runner_temp.join("npm");
        let tools = root.path().join("tools");
        fs::create_dir_all(&npm_dir)?;
        fs::create_dir_all(&tools)?;

        let case = Self {
            curl_marker: root.path().join("curl-called"),
            curl_args: root.path().join("curl-args"),
            curl_config: root.path().join("curl-config"),
            curl_body: root.path().join("curl-body"),
            digest_marker: root.path().join("digest-called"),
            encoder_marker: root.path().join("encoder-called"),
            runner_temp,
            root,
        };
        Self::install_curl_recorder(&tools)?;
        Ok(case)
    }

    fn archive_path(&self) -> PathBuf {
        self.runner_temp
            .join("npm")
            .join(format!("fixture-web-{VERSION}.tgz"))
    }

    fn write_archive(&self) -> Result<PathBuf, Box<dyn Error>> {
        let source = self.root.path().join("archive-source/package");
        fs::create_dir_all(&source)?;
        fs::write(
            source.join("package.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": MEMBER,
                "version": VERSION,
                "publishConfig": {"registry": "https://npm.pkg.github.com"},
                "customMetadata": "preserved"
            }))?,
        )?;
        fs::write(source.join("index.js"), b"export const value = 42;\n")?;

        let archive = self.archive_path();
        let output = Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(source.parent().ok_or("archive source has no parent")?)
            .arg("package")
            .output()?;
        ensure_success("create package archive", &output)?;
        Ok(archive)
    }

    fn run(&self, script: &str, openssl: Option<&Path>) -> Result<Output, Box<dyn Error>> {
        let tools = self.root.path().join("tools");
        let mut paths = vec![tools];
        if let Some(openssl) = openssl {
            paths.insert(
                0,
                openssl
                    .parent()
                    .ok_or("openssl has no parent")?
                    .to_path_buf(),
            );
        }
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").ok_or("PATH missing")?,
        ));
        let path = std::env::join_paths(paths)?;
        Ok(Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(self.runner_temp.join("npm"))
            .env_clear()
            .env("PATH", path)
            .env("RUNNER_TEMP", &self.runner_temp)
            .env("GITHUB_REF_NAME", "fixture-v1.2.3")
            .env("GH_TOKEN", "fixture-token")
            .env("CURL_MARKER", &self.curl_marker)
            .env("CURL_ARGS", &self.curl_args)
            .env("CURL_CONFIG", &self.curl_config)
            .env("CURL_BODY", &self.curl_body)
            .env("DIGEST_MARKER", &self.digest_marker)
            .env("ENCODER_MARKER", &self.encoder_marker)
            .output()?)
    }

    fn install_curl_recorder(tools: &Path) -> Result<(), Box<dyn Error>> {
        let curl = tools.join("curl");
        fs::write(
            &curl,
            "#!/bin/sh\nprintf '%s\\n' called > \"$CURL_MARKER\"\nfor arg do\n  printf '%s\\n' \"$arg\" >> \"$CURL_ARGS\"\n  case \"$arg\" in @*) cat \"${arg#@}\" > \"$CURL_BODY\" ;; esac\ndone\ncat > \"$CURL_CONFIG\"\n",
        )?;
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    fn install_digest_failure_openssl(&self) -> Result<PathBuf, Box<dyn Error>> {
        let tools = self.root.path().join("openssl-tools");
        fs::create_dir_all(&tools)?;
        let wrapper = tools.join("openssl");
        let real_openssl = find_host_executable("openssl")?;
        fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\ncase \"$1\" in\ndgst) printf '%s\\n' called > \"$DIGEST_MARKER\"; printf digest-bytes; exit 42 ;;\nbase64) printf '%s\\n' called > \"$ENCODER_MARKER\"; exec '{}' \"$@\" ;;\n*) exec '{}' \"$@\" ;;\nesac\n",
                real_openssl.display(),
                real_openssl.display()
            ),
        )?;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;
        Ok(wrapper)
    }
}

fn generated_publish_script() -> Result<String, Box<dyn Error>> {
    let repo = make_bun_repo()?;
    write_bun_fixture(repo.path())?;
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree(&prepared)?;
    let workflow = rendered
        .get(PUBLISH_WORKFLOW_PATH)
        .ok_or("rendered package publication workflow is missing")?;
    if !workflow.contains("Publish to GitHub Packages") {
        return Err("rendered publish step is missing".into());
    }

    let section = workflow
        .split("- name: Publish to GitHub Packages")
        .nth(1)
        .ok_or("rendered publish step is missing")?;
    let run = section
        .lines()
        .take_while(|line| !line.trim_start().starts_with("- name: "))
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or("rendered publish command is missing")?;
    let command: String = serde_json::from_str(run)?;
    if !command.contains("Publish to GitHub Packages") && !command.contains("openssl dgst") {
        return Err("rendered package publisher command is unexpected".into());
    }
    Ok(command)
}

fn find_host_executable(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH missing")?) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("host executable not found: {name}").into())
}

fn ensure_success(label: &str, output: &Output) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{label} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}
