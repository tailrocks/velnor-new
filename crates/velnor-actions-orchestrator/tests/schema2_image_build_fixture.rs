use super::TestResult;

#[path = "schema2_image_build_fixture_mocks.rs"]
mod mocks;
use mocks::{DOCKER_MOCK, FILE_MOCK, MISE_MOCK};

struct Fixture {
    temporary: tempfile::TempDir,
    workspace: std::path::PathBuf,
    bin: std::path::PathBuf,
    target: std::path::PathBuf,
    storage_init: std::path::PathBuf,
    job_start_hook: std::path::PathBuf,
    probe_sha: String,
    init_sha: String,
    hook_sha: String,
    wrapper_sha: String,
}

struct Scenario {
    name: &'static str,
    head: &'static str,
    tracked_change: &'static str,
    untracked_path: &'static str,
    utility_kind: &'static str,
    hook_kind: &'static str,
    image_sha: &'static str,
    image_mode: &'static str,
    hook_sha: &'static str,
    hook_mode: &'static str,
    hook_owner: &'static str,
}

pub(super) fn run() -> TestResult {
    let fixture = prepare()?;
    let good = Scenario::good();
    assert!(!fixture.storage_init.exists());
    let output = invoke(&fixture, &good)?;
    assert!(
        output.status.success(),
        "The image build fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_build_was_run(&fixture, &good)?;

    for scenario in source_failures() {
        assert_rejected_before_build(&fixture, &scenario)?;
    }
    for scenario in binary_failures() {
        assert_rejected_before_docker(&fixture, &scenario)?;
    }
    for scenario in hook_failures() {
        assert_rejected_before_docker(&fixture, &scenario)?;
    }
    for scenario in image_failures() {
        let output = invoke(&fixture, &scenario)?;
        assert!(!output.status.success());
        assert_build_was_run(&fixture, &scenario)?;
        assert_ne!(docker_calls(&fixture, &scenario)?, "");
    }
    Ok(())
}

fn prepare() -> Result<Fixture, Box<dyn std::error::Error>> {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("The workspace root is missing.")?
        .to_path_buf();
    let temporary = tempfile::tempdir()?;
    let bin = temporary.path().join("bin");
    let target = temporary.path().join("target");
    let release = target.join("x86_64-unknown-linux-musl/release");
    std::fs::create_dir_all(&bin)?;
    std::fs::create_dir_all(&release)?;
    let probe = release.join("velnor-resource-probe");
    let storage_init = release.join("velnor-storage-init");
    let job_start_hook = release.join("velnor-job-start-hook");
    let revision = "0123456789012345678901234567890123456789";
    std::fs::write(&probe, format!("source {revision}"))?;
    let payload = temporary.path().join("utility-payload");
    std::fs::write(&payload, b"utility")?;
    let init_sha = sha256(&payload)?;
    let hook_sha = sha256(&payload)?;
    let wrapper_sha = sha256(&workspace.join("images/runner/ubuntu-26.04/job-started-hook.sh"))?;
    std::fs::remove_file(payload)?;
    install_mocks(&bin)?;
    Ok(Fixture {
        temporary,
        workspace,
        bin,
        target,
        storage_init,
        job_start_hook,
        probe_sha: sha256(&probe)?,
        init_sha,
        hook_sha,
        wrapper_sha,
    })
}

fn install_mocks(bin: &std::path::Path) -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    for (name, source) in [
        ("git", GIT_MOCK),
        ("mise", MISE_MOCK),
        ("file", FILE_MOCK),
        ("docker", DOCKER_MOCK),
    ] {
        let path = bin.join(name);
        std::fs::write(&path, source)?;
        let mut mode = std::fs::metadata(&path)?.permissions();
        mode.set_mode(0o755);
        std::fs::set_permissions(path, mode)?;
    }
    Ok(())
}

fn invoke(fixture: &Fixture, scenario: &Scenario) -> std::io::Result<std::process::Output> {
    let previous_path = std::env::var("PATH").unwrap_or_default();
    let mut command = std::process::Command::new("bash");
    command
        .arg(
            fixture
                .workspace
                .join("images/resource-probe/build-runner-images.sh"),
        )
        .current_dir(&fixture.workspace)
        .env("PATH", format!("{}:{previous_path}", fixture.bin.display()))
        .env("CARGO_TARGET_DIR", &fixture.target)
        .env("MOCK_HEAD", scenario.head)
        .env("MOCK_TRACKED_CHANGE", scenario.tracked_change)
        .env("MOCK_UNTRACKED_PATH", scenario.untracked_path)
        .env("MOCK_STORAGE_INIT", &fixture.storage_init)
        .env("MOCK_JOB_START_HOOK", &fixture.job_start_hook)
        .env("MOCK_UTILITY_KIND", scenario.utility_kind)
        .env("MOCK_HOOK_KIND", scenario.hook_kind)
        .env("VELNOR_SOURCE_REVISION", SOURCE_REVISION)
        .env("RUNNER_TEMP", fixture.temporary.path())
        .env("DOCKER_CALLS", docker_path(fixture, scenario))
        .env("MISE_CALLS", mise_path(fixture, scenario))
        .env("TRACE", trace_path(fixture, scenario))
        .env("EXPECTED_PROBE_SHA", &fixture.probe_sha)
        .env("EXPECTED_INIT_SHA", &fixture.init_sha)
        .env("ACTUAL_INIT_SHA", scenario.image_sha)
        .env("ACTUAL_INIT_MODE", scenario.image_mode)
        .env("EXPECTED_HOOK_SHA", &fixture.hook_sha)
        .env("ACTUAL_HOOK_SHA", scenario.hook_sha)
        .env("ACTUAL_HOOK_MODE", scenario.hook_mode)
        .env("ACTUAL_HOOK_OWNER", scenario.hook_owner)
        .env("EXPECTED_WRAPPER_SHA", &fixture.wrapper_sha);
    command.output()
}

fn assert_build_was_run(fixture: &Fixture, scenario: &Scenario) -> TestResult {
    assert_mise_invoked(fixture, scenario)?;
    assert!(fixture.storage_init.is_file());
    assert_eq!(std::fs::read(&fixture.storage_init)?, b"utility");
    assert!(fixture.job_start_hook.is_file());
    assert_eq!(std::fs::read(&fixture.job_start_hook)?, b"utility");
    let trace = std::fs::read_to_string(trace_path(fixture, scenario))?;
    let mut events = trace.lines();
    assert_eq!(events.next(), Some("mise"));
    assert!(events.all(|event| event == "docker"));
    Ok(())
}

fn assert_rejected_before_build(fixture: &Fixture, scenario: &Scenario) -> TestResult {
    let output = invoke(fixture, scenario)?;
    assert!(!output.status.success());
    assert!(!mise_path(fixture, scenario).exists());
    assert_eq!(docker_calls(fixture, scenario)?, "");
    assert!(!trace_path(fixture, scenario).exists());
    Ok(())
}

fn assert_rejected_before_docker(fixture: &Fixture, scenario: &Scenario) -> TestResult {
    let output = invoke(fixture, scenario)?;
    assert!(!output.status.success());
    assert_mise_invoked(fixture, scenario)?;
    assert_eq!(docker_calls(fixture, scenario)?, "");
    Ok(())
}

fn assert_mise_invoked(fixture: &Fixture, scenario: &Scenario) -> TestResult {
    let calls = std::fs::read_to_string(mise_path(fixture, scenario))?;
    let expected = "--no-config --no-env --no-hooks exec rust@1.99.1 -- cargo build --locked --manifest-path crates/velnor-runner/Cargo.toml --package velnor-resource-probe --bin velnor-storage-init --bin velnor-job-start-hook --release --target x86_64-unknown-linux-musl\n";
    assert_eq!(calls, expected);
    let trace = std::fs::read_to_string(trace_path(fixture, scenario))?;
    assert_eq!(trace.lines().next(), Some("mise"));
    Ok(())
}

fn source_failures() -> [Scenario; 5] {
    [
        Scenario {
            name: "wrong-head",
            head: OTHER_REVISION,
            ..Scenario::good()
        },
        Scenario {
            name: "index-dirty",
            tracked_change: "index",
            ..Scenario::good()
        },
        Scenario {
            name: "worktree-dirty",
            tracked_change: "worktree",
            ..Scenario::good()
        },
        Scenario {
            name: "untracked-rust",
            untracked_path: "crates/velnor-resource-probe/src/new.rs",
            ..Scenario::good()
        },
        Scenario {
            name: "untracked-build",
            untracked_path: "images/resource-probe/build.sh",
            ..Scenario::good()
        },
    ]
}

fn binary_failures() -> [Scenario; 4] {
    [
        Scenario {
            name: "missing-binary",
            utility_kind: "missing",
            ..Scenario::good()
        },
        Scenario {
            name: "wrong-architecture",
            utility_kind: "wrong-arch",
            ..Scenario::good()
        },
        Scenario {
            name: "dynamic-binary",
            utility_kind: "dynamic",
            ..Scenario::good()
        },
        Scenario {
            name: "bad-binary",
            utility_kind: "bad",
            ..Scenario::good()
        },
    ]
}

fn hook_failures() -> [Scenario; 4] {
    [
        Scenario {
            name: "missing-hook",
            hook_kind: "missing",
            ..Scenario::good()
        },
        Scenario {
            name: "wrong-hook-architecture",
            hook_kind: "wrong-arch",
            ..Scenario::good()
        },
        Scenario {
            name: "dynamic-hook",
            hook_kind: "dynamic",
            ..Scenario::good()
        },
        Scenario {
            name: "bad-hook",
            hook_kind: "bad",
            ..Scenario::good()
        },
    ]
}

fn image_failures() -> [Scenario; 5] {
    [
        Scenario {
            name: "wrong-image-hash",
            image_sha: "0",
            ..Scenario::good()
        },
        Scenario {
            name: "wrong-image-mode",
            image_mode: "755",
            ..Scenario::good()
        },
        Scenario {
            name: "wrong-hook-hash",
            hook_sha: "0",
            ..Scenario::good()
        },
        Scenario {
            name: "wrong-hook-owner",
            hook_owner: "1:1",
            ..Scenario::good()
        },
        Scenario {
            name: "wrong-hook-mode",
            hook_mode: "755",
            ..Scenario::good()
        },
    ]
}

impl Scenario {
    fn good() -> Self {
        Self {
            name: "good",
            head: SOURCE_REVISION,
            tracked_change: "clean",
            untracked_path: "",
            utility_kind: "static",
            hook_kind: "static",
            image_sha: "good",
            image_mode: "555",
            hook_sha: "good",
            hook_mode: "555",
            hook_owner: "0:0",
        }
    }
}

fn sha256(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()?;
    assert!(output.status.success());
    String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "sha256 output is empty".into())
}

fn docker_path(fixture: &Fixture, scenario: &Scenario) -> std::path::PathBuf {
    fixture
        .temporary
        .path()
        .join(format!("docker-{}.log", scenario.name))
}

fn mise_path(fixture: &Fixture, scenario: &Scenario) -> std::path::PathBuf {
    fixture
        .temporary
        .path()
        .join(format!("mise-{}.log", scenario.name))
}

fn trace_path(fixture: &Fixture, scenario: &Scenario) -> std::path::PathBuf {
    fixture
        .temporary
        .path()
        .join(format!("trace-{}.log", scenario.name))
}

fn docker_calls(fixture: &Fixture, scenario: &Scenario) -> std::io::Result<String> {
    std::fs::read_to_string(docker_path(fixture, scenario)).or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(String::new())
        } else {
            Err(error)
        }
    })
}

const SOURCE_REVISION: &str = "0123456789012345678901234567890123456789";
const OTHER_REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const GIT_MOCK: &str = r#"#!/bin/bash
case "$*" in
  'rev-parse HEAD') printf '%s\n' "$MOCK_HEAD" ;;
  'diff --quiet HEAD --') [[ "$MOCK_TRACKED_CHANGE" == clean ]] ;;
  'ls-files --others --exclude-standard') [[ -z "$MOCK_UNTRACKED_PATH" ]] || printf '%s\n' "$MOCK_UNTRACKED_PATH" ;;
  *) exit 97 ;;
esac
"#;
