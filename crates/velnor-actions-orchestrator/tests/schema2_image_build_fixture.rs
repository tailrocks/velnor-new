use super::TestResult;

struct Fixture {
    temporary: tempfile::TempDir,
    workspace: std::path::PathBuf,
    bin: std::path::PathBuf,
    target: std::path::PathBuf,
    storage_init: std::path::PathBuf,
    probe_sha: String,
    init_sha: String,
}

struct Scenario {
    name: &'static str,
    head: &'static str,
    tracked_change: &'static str,
    untracked_path: &'static str,
    utility_kind: &'static str,
    image_sha: &'static str,
    image_mode: &'static str,
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
    let revision = "0123456789012345678901234567890123456789";
    std::fs::write(&probe, format!("source {revision}"))?;
    let payload = temporary.path().join("utility-payload");
    std::fs::write(&payload, b"utility")?;
    let init_sha = sha256(&payload)?;
    std::fs::remove_file(payload)?;
    install_mocks(&bin)?;
    Ok(Fixture {
        temporary,
        workspace,
        bin,
        target,
        storage_init,
        probe_sha: sha256(&probe)?,
        init_sha,
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
        .env("MOCK_UTILITY_KIND", scenario.utility_kind)
        .env("VELNOR_SOURCE_REVISION", SOURCE_REVISION)
        .env("RUNNER_TEMP", fixture.temporary.path())
        .env("DOCKER_CALLS", docker_path(fixture, scenario))
        .env("MISE_CALLS", mise_path(fixture, scenario))
        .env("TRACE", trace_path(fixture, scenario))
        .env("EXPECTED_PROBE_SHA", &fixture.probe_sha)
        .env("EXPECTED_INIT_SHA", &fixture.init_sha)
        .env("ACTUAL_INIT_SHA", scenario.image_sha)
        .env("ACTUAL_INIT_MODE", scenario.image_mode);
    command.output()
}

fn assert_build_was_run(fixture: &Fixture, scenario: &Scenario) -> TestResult {
    assert_mise_invoked(fixture, scenario)?;
    assert!(fixture.storage_init.is_file());
    assert_eq!(std::fs::read(&fixture.storage_init)?, b"utility");
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
    let expected = "--no-config --no-env --no-hooks exec rust@1.99.1 -- cargo build --locked --manifest-path crates/velnor-runner/Cargo.toml --package velnor-resource-probe --bin velnor-storage-init --release --target x86_64-unknown-linux-musl\n";
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

fn image_failures() -> [Scenario; 2] {
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
            image_sha: "good",
            image_mode: "555",
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

const MISE_MOCK: &str = r#"#!/bin/bash
printf '%s\n' "$*" >> "$MISE_CALLS"
printf 'mise\n' >> "$TRACE"
[[ "$*" == '--no-config --no-env --no-hooks exec rust@1.99.1 -- cargo build --locked --manifest-path crates/velnor-runner/Cargo.toml --package velnor-resource-probe --bin velnor-storage-init --release --target x86_64-unknown-linux-musl' ]] || exit 97
mkdir -p "$(dirname "$MOCK_STORAGE_INIT")"
rm -f "$MOCK_STORAGE_INIT"
[[ "$MOCK_UTILITY_KIND" == missing ]] || printf utility > "$MOCK_STORAGE_INIT"
"#;

const FILE_MOCK: &str = r#"#!/bin/bash
case "$*:$MOCK_UTILITY_KIND" in
  *velnor-storage-init:wrong-arch) printf 'Mach-O arm64\n' ;;
  *velnor-storage-init:dynamic) printf 'ELF 64-bit LSB pie executable, x86-64, dynamically linked\n' ;;
  *velnor-storage-init:bad) printf 'data\n' ;;
  *) printf 'ELF 64-bit LSB pie executable, x86-64, static-pie linked\n' ;;
esac
"#;

const DOCKER_MOCK: &str = r#"#!/bin/bash
printf 'docker\n' >> "$TRACE"
printf '<%s>' "$@" >> "$DOCKER_CALLS"
printf '\n' >> "$DOCKER_CALLS"
previous=''
for argument in "$@"; do
  if [[ "$previous" == '--format' && "$argument" == '{{.Architecture}}' ]]; then printf 'amd64\n'; fi
  if [[ "$previous" == '--format' && "$argument" == '{{.Id}}' ]]; then printf 'sha256:%064d\n' 0; fi
  if [[ "$previous" == '--entrypoint' && "$argument" == '/usr/bin/uname' ]]; then printf 'x86_64\n'; fi
  if [[ "$previous" == '--entrypoint' && "$argument" == '/usr/bin/sha256sum' ]]; then entrypoint=sha256sum; fi
  if [[ "$previous" == '--entrypoint' && "$argument" == '/usr/bin/stat' ]]; then entrypoint=stat; fi
  previous="$argument"
done
case " $* " in
  *'/velnor/storage-init'*)
    if [[ "$entrypoint" == sha256sum ]]; then
      if [[ "$ACTUAL_INIT_SHA" == good ]]; then printf '%s  /velnor/storage-init\n' "$EXPECTED_INIT_SHA"; else printf '%s  /velnor/storage-init\n' "$ACTUAL_INIT_SHA"; fi
    elif [[ "$entrypoint" == stat ]]; then printf '%s\n' "$ACTUAL_INIT_MODE"; fi
    ;;
  *'/velnor/resource-probe'*)
    [[ "$entrypoint" == sha256sum ]] && printf '%s  /velnor/resource-probe\n' "$EXPECTED_PROBE_SHA"
    ;;
esac
"#;
