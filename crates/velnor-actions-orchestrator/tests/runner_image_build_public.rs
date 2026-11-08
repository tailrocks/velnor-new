// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use velnor_actions_mise::{CheckDeadline, IsolatedCommand, MiseError, ProcessOutput};
use velnor_actions_orchestrator::{
    BuildRunnerImagesRequest, RunnerImageBuildExecution, RunnerImageBuildPurpose,
    build_runner_images_with_execution,
};

#[path = "runner_image_build_public/archive.rs"]
mod archive;
#[path = "runner_image_build_public/output.rs"]
mod output;
#[path = "runner_image_build_public/records.rs"]
mod records;
#[path = "runner_image_build_public/source.rs"]
mod source;

struct ExternalBuildx {
    plugin: PathBuf,
    mutate_context: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}
impl RunnerImageBuildExecution for ExternalBuildx {
    fn run(&self, command: &IsolatedCommand, _: CheckDeadline) -> Result<ProcessOutput, MiseError> {
        let result = self.execute(command).map_err(|e| MiseError::SpawnFailed {
            program: "docker".into(),
            message: e.to_string(),
        })?;
        Ok(ProcessOutput {
            stdout: result.into_bytes(),
            stderr: Vec::new(),
            code: Some(0),
            signal: None,
            success: true,
        })
    }
}
impl ExternalBuildx {
    fn execute(&self, command: &IsolatedCommand) -> Result<String, Box<dyn Error>> {
        let argv = command
            .argv()
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if argv.iter().any(|v| v == "version") {
            return Ok("github.com/docker/buildx v0.38.0 fixture\n".into());
        }
        if argv.iter().any(|v| v == "info") {
            return Ok(
                serde_json::json!([{"Name":"buildx","Version":"v0.38.0","Path":self.plugin}])
                    .to_string(),
            );
        }
        let output = argv
            .iter()
            .find_map(|v| v.strip_prefix("type=oci,dest="))
            .ok_or("output")?
            .split(',')
            .next()
            .ok_or("output path")?;
        let role = argv
            .iter()
            .find_map(|v| match v.as_str() {
                "velnor-runner:linux-amd64" => Some("runner"),
                "velnor-dind:linux-amd64" => Some("dind"),
                "velnor-resource-probe:linux-amd64" => Some("resource_probe"),
                _ => None,
            })
            .ok_or("role")?;
        let prefix = if role == "resource_probe" {
            "SOURCE_SHA="
        } else {
            "VELNOR_SOURCE_SHA="
        };
        let source = argv
            .windows(2)
            .find_map(|pair| {
                (pair[0] == "--build-arg")
                    .then(|| pair[1].strip_prefix(prefix))
                    .flatten()
            })
            .unwrap_or("");
        if let Some(witness) = &self.mutate_context {
            output::mutate_readonly_context(
                &command
                    .cwd()
                    .ok_or("missing build root")?
                    .join("target/velnor-runner-family/resource_probe/resource-probe"),
            )?;
            witness.store(true, std::sync::atomic::Ordering::Release);
        }
        let root = command.cwd().ok_or("missing build root")?;
        let revision = git(root, &["rev-parse", "HEAD"])?;
        fs::write(output, archive::make(role, source, revision.trim())?)?;
        Ok(String::new())
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    runner_temp: PathBuf,
    contexts: PathBuf,
    output: PathBuf,
    sha: String,
    plugin: PathBuf,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let physical = temp.path().canonicalize()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&physical, fs::Permissions::from_mode(0o700))?;
        }
        let root = physical.join("source");
        let runner_temp = physical.join("runner");
        let contexts = physical.join("contexts");
        let output = physical.join("family");
        for path in [&root, &runner_temp, &contexts] {
            fs::create_dir(path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            }
        }
        git(&root, &["init", "--quiet"])?;
        git(&root, &["config", "user.name", "Alexey Zhokhov"])?;
        git(&root, &["config", "user.email", "alexey@zhokhov.com"])?;
        git(&root, &["config", "commit.gpgsign", "false"])?;
        let plugin =
            runner_temp.join(".velnor-job-image-candidate/docker-config/cli-plugins/docker-buildx");
        fs::create_dir_all(plugin.parent().ok_or("plugin parent")?)?;
        fs::write(&plugin, b"fixture buildx")?;
        let hash = hex(&Sha256::digest(b"fixture buildx"));
        let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for path in velnor_actions_contract::runner_publication_source_inputs()? {
            let destination = root.join(path);
            fs::create_dir_all(destination.parent().ok_or("parent")?)?;
            let data = if *path == ".velnor/runner.lock" {
                let lock = fs::read_to_string(source_root.join(path))?;
                let old = lock
                    .lines()
                    .find(|line| line.starts_with("buildx_sha256 = "))
                    .ok_or("lock hash")?;
                lock.replace(old, &format!("buildx_sha256 = \"{hash}\""))
            } else if path.ends_with("Dockerfile") {
                fs::read_to_string(source_root.join(path))?
            } else {
                format!("fixture {path}\n")
            };
            if *path == "scripts/check-runner-image-candidate.sh" {
                fs::write(destination, b"binary\0source\nbytes\xff\n")?;
            } else {
                fs::write(destination, data)?;
            }
        }
        fs::write(root.join(".gitignore"), "target/\n")?;
        git(&root, &["add", "--all"])?;
        git(
            &root,
            &[
                "commit",
                "-s",
                "--quiet",
                "-m",
                "fixture\n\nCo-authored-by: Codex <codex@openai.com>",
            ],
        )?;
        let sha = git(&root, &["rev-parse", "HEAD"])?.trim().to_owned();
        for name in [
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ] {
            fs::write(contexts.join(name), format!("fixture {name}"))?;
        }
        Ok(Self {
            _temp: temp,
            root,
            runner_temp,
            contexts,
            output,
            sha,
            plugin,
        })
    }
    fn request(&self) -> Result<BuildRunnerImagesRequest, Box<dyn Error>> {
        Ok(BuildRunnerImagesRequest {
            root: self.root.clone(),
            source_commit: self.sha.clone(),
            runner_temp: self.runner_temp.clone(),
            contexts_dir: self.contexts.clone(),
            output_dir: self.output.clone(),
            purpose: RunnerImageBuildPurpose::Qualification,
            deadline: CheckDeadline::after(Duration::from_secs(120))?,
        })
    }
    fn execution(&self) -> ExternalBuildx {
        ExternalBuildx {
            plugin: self.plugin.clone(),
            mutate_context: None,
        }
    }
}
fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let out = Command::new("git").current_dir(root).args(args).output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string().into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

fn frozen_clock<T>(operation: impl FnOnce() -> T) -> T {
    let now = std::time::Instant::now();
    CheckDeadline::with_clock(move || now, operation)
}

#[test]
fn builds_the_three_role_family_from_committed_source() -> Result<(), Box<dyn Error>> {
    frozen_clock(|| {
        let fixture = Fixture::new()?;
        let mut wrong = fixture.request()?;
        wrong.source_commit = "a".repeat(40);
        assert!(
            build_runner_images_with_execution(&wrong, &fixture.execution()).is_err(),
            "wrong source accepted"
        );
        assert!(!fixture.output.exists());
        wrong.source_commit.clear();
        assert!(
            build_runner_images_with_execution(&wrong, &fixture.execution()).is_err(),
            "missing source accepted"
        );
        assert!(!fixture.output.exists());
        let foreign = fixture.contexts.join("foreign");
        fs::write(&foreign, b"foreign canary")?;
        assert!(
            build_runner_images_with_execution(&fixture.request()?, &fixture.execution()).is_err(),
            "extra context accepted"
        );
        assert_eq!(fs::read(&foreign)?, b"foreign canary");
        assert!(!fixture.output.exists());
        fs::remove_file(foreign)?;
        let staged = fixture.root.join("target/velnor-runner-family");
        fs::create_dir_all(&staged)?;
        fs::write(staged.join("foreign"), b"foreign canary")?;
        assert!(
            build_runner_images_with_execution(&fixture.request()?, &fixture.execution()).is_err(),
            "foreign stage accepted"
        );
        assert_eq!(fs::read(staged.join("foreign"))?, b"foreign canary");
        assert!(!fixture.output.exists());
        fs::remove_file(staged.join("foreign"))?;
        fs::remove_dir(staged)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = fixture.contexts.join("storage_init");
            fs::remove_file(&link)?;
            symlink(fixture.contexts.join("resource_probe"), &link)?;
            assert!(
                build_runner_images_with_execution(&fixture.request()?, &fixture.execution())
                    .is_err(),
                "symlink context accepted"
            );
            assert!(!fixture.output.exists());
            fs::remove_file(&link)?;
            fs::write(link, b"fixture storage_init")?;
        }
        build_runner_images_with_execution(&fixture.request()?, &fixture.execution())?;
        for name in [
            "velnor-runner-linux-amd64.tar",
            "velnor-dind-linux-amd64.tar",
            "velnor-resource-probe-linux-amd64.tar",
            "runner-image-provenance.json",
            "dind-image-provenance.json",
            "resource-probe-image-provenance.json",
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ] {
            assert!(fixture.output.join(name).is_file(), "missing {name}");
        }
        assert_eq!(fs::read_dir(&fixture.output)?.count(), 10);
        Ok(())
    })
}

#[test]
fn publication_build_requires_main_push_and_writes_three_bound_records()
-> Result<(), Box<dyn Error>> {
    frozen_clock(|| {
        if let Some(root) = std::env::var_os("VELNOR_TEST_FAMILY_ROOT") {
            let root = PathBuf::from(root);
            let sha = std::env::var("GITHUB_SHA")?;
            let runner_temp = root.join("runner");
            let execution = ExternalBuildx {
                mutate_context: None,
                plugin: runner_temp
                    .join(".velnor-job-image-candidate/docker-config/cli-plugins/docker-buildx"),
            };
            let request = BuildRunnerImagesRequest {
                root: root.join("source"),
                source_commit: sha,
                runner_temp,
                contexts_dir: root.join("contexts"),
                output_dir: root.join("family"),
                purpose: RunnerImageBuildPurpose::Publication,
                deadline: CheckDeadline::after(Duration::from_secs(120))?,
            };
            build_runner_images_with_execution(&request, &execution)?;
            return Ok(());
        }
        let fixture = Fixture::new()?;
        let invoke = |event: &str| -> Result<std::process::Output, Box<dyn Error>> {
            Ok(Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "publication_build_requires_main_push_and_writes_three_bound_records",
                    "--nocapture",
                ])
                .env(
                    "VELNOR_TEST_FAMILY_ROOT",
                    fixture.root.parent().ok_or("fixture parent")?,
                )
                .env("GITHUB_SHA", &fixture.sha)
                .env("GITHUB_WORKFLOW_SHA", &fixture.sha)
                .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
                .env("GITHUB_REPOSITORY_ID", "1390620900")
                .env("GITHUB_EVENT_NAME", event)
                .env("GITHUB_REF", "refs/heads/main")
                .env(
                    "GITHUB_WORKFLOW_REF",
                    "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
                )
                .env("GITHUB_RUN_ID", "31")
                .env("GITHUB_RUN_ATTEMPT", "2")
                .output()?)
        };
        let denied = invoke("pull_request")?;
        assert!(
            !denied.status.success(),
            "pull request minted publication records"
        );
        assert!(!fixture.output.exists());
        let accepted = invoke("push")?;
        assert!(
            accepted.status.success(),
            "publication failed: {}",
            String::from_utf8_lossy(&accepted.stderr)
        );
        assert_eq!(fs::read_dir(&fixture.output)?.count(), 13);
        records::assert_publication_records(&fixture)?;
        Ok(())
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    result
}

#[test]
fn runner_family_budget_allows_a_slow_build_and_expires_at_its_bound() -> Result<(), Box<dyn Error>>
{
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use velnor_actions_contract::config::RUNNER_IMAGE_FAMILY_BUILD_TIMEOUT_MINUTES;
    for (elapsed, expected) in [
        (11, true),
        (
            u64::from(RUNNER_IMAGE_FAMILY_BUILD_TIMEOUT_MINUTES) + 1,
            false,
        ),
    ] {
        let fixture = frozen_clock(Fixture::new)?;
        let mut request = frozen_clock(|| fixture.request())?;
        let now = std::time::Instant::now();
        let calls = Arc::new(AtomicUsize::new(0));
        let ticks = Arc::clone(&calls);
        CheckDeadline::with_clock(
            move || {
                if ticks.fetch_add(1, Ordering::SeqCst) == 0 {
                    now
                } else {
                    now + Duration::from_mins(elapsed)
                }
            },
            || {
                request.deadline = CheckDeadline::after(Duration::from_mins(u64::from(
                    RUNNER_IMAGE_FAMILY_BUILD_TIMEOUT_MINUTES,
                )))?;
                let result = build_runner_images_with_execution(&request, &fixture.execution());
                assert_eq!(result.is_ok(), expected, "{result:?}");
                Ok::<(), Box<dyn Error>>(())
            },
        )?;
    }
    Ok(())
}
