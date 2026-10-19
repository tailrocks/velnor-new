use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
    ValidatedImageReference,
};
use velnor_actions_contract::{
    Concurrency, ExecutionMode, Job, JobTimeout, Permissions, ReleaseTarget, ScaleSetSelector,
    Step, StepKind, Trigger, WorkflowIr,
};
use velnor_actions_workflow_renderer::WorkflowRoutingPolicy;
use velnor_actions_workflow_renderer::render::RenderedFile;

use super::validate_final_routing;

const IMAGE: &str = "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )
    .expect("selector is valid")
}

fn policy(mode: ExecutionMode, with_image: bool) -> WorkflowRoutingPolicy {
    let image = with_image.then(|| ValidatedImageReference::parse(IMAGE).expect("image is valid"));
    WorkflowRoutingPolicy::configured(mode, selector(), image)
}

fn native_ir() -> WorkflowIr {
    check_ir(
        "native-scale-set",
        CheckRunner {
            label: "native-scale-set".to_owned(),
            platform: CheckPlatform::MacosArm64,
            executor: CheckExecutor::EphemeralSelfHosted,
            container: None,
        },
    )
}

fn scale_set_ir() -> WorkflowIr {
    check_ir(
        &selector().token(),
        CheckRunner {
            label: "linux-check".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::EphemeralSelfHosted,
            container: None,
        },
    )
}

fn check_ir(runs_on: &str, runner: CheckRunner) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs: BTreeMap::from([(
            "check-native".to_owned(),
            Job {
                display_name: "Check / native".to_owned(),
                runs_on: runs_on.to_owned(),
                check_runner: Some(runner),
                timeout_minutes: JobTimeout::VALIDATOR,
                needs: Vec::new(),
                condition: Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned()),
                permissions: None,
                cache_mode: None,
                environment: None,
                steps: vec![Step {
                    name: "Run check".to_owned(),
                    id: None,
                    role: None,
                    condition: None,
                    kind: StepKind::Shell {
                        run: vec!["true".to_owned()],
                        env: BTreeMap::new(),
                    },
                }],
            },
        )]),
    }
}

fn file(runs_on: &str, container: Option<&str>) -> RenderedFile {
    let container = container.map_or_else(String::new, |image| {
        format!("    container:\n      image: {image}\n")
    });
    RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: format!(
            "on: push\njobs:\n  check-native:\n    runs-on: {runs_on}\n    if: \"{EPHEMERAL_CHECK_ADMISSION_CONDITION}\"\n{container}    steps: []\n"
        ),
    }
}

#[test]
fn final_generator_audit_checks_typed_native_and_scale_set_jobs() {
    let native = native_ir();
    let native_file = file("native-scale-set", None);
    assert!(
        validate_final_routing(
            std::slice::from_ref(&native_file),
            &policy(ExecutionMode::Hosted, false),
            &native,
        )
        .is_ok()
    );
    assert!(
        validate_final_routing(
            std::slice::from_ref(&native_file),
            &policy(ExecutionMode::ScaleSet, true),
            &native,
        )
        .is_err()
    );
    assert!(
        validate_final_routing(
            &[file("another-runner", None)],
            &policy(ExecutionMode::Hosted, false),
            &native,
        )
        .is_err()
    );

    let scale_set = scale_set_ir();
    let scale_set_file = file(&selector().token(), Some(IMAGE));
    assert!(
        validate_final_routing(
            &[scale_set_file],
            &policy(ExecutionMode::Both, true),
            &scale_set,
        )
        .is_ok()
    );
}

#[test]
fn full_generator_renders_typed_native_check_before_its_final_audit() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = isolated_generation_root(&source);
    let mut prep = crate::prepare(root.path()).expect("the repository has a valid generation plan");
    let execution = prep
        .config
        .execution
        .as_mut()
        .expect("execution profiles are configured");
    let scale_set_profile = execution.scale_set_profile.clone();
    execution
        .profiles
        .get_mut(&scale_set_profile)
        .expect("Scale Set profile exists")
        .job_container_image = Some(ValidatedImageReference::parse(IMAGE).expect("fixture image"));
    let job = prep
        .workflow
        .ir
        .jobs
        .get_mut("check-maintenance-helpers")
        .expect("the maintenance check exists");
    job.runs_on = "native-scale-set".to_owned();
    job.condition = Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned());
    job.check_runner = Some(CheckRunner {
        label: "native-scale-set".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container: None,
    });

    let owned =
        crate::finalized::owned_preparation(&prep).expect("production helper attachment succeeds");
    let tree =
        super::render_all(&owned, None).expect("the generator accepts its typed native check");
    let ci = tree
        .files
        .iter()
        .find(|file| file.path == ".github/workflows/ci.yml")
        .expect("CI workflow is generated");
    assert!(ci.bytes.contains("runs-on: native-scale-set"));
    assert!(ci.bytes.contains(EPHEMERAL_CHECK_ADMISSION_CONDITION));
    assert!(tree.files.iter().any(|file| {
        file.bytes
            .contains("runs-on: [velnor, ubuntu-26.04-scale-set]")
            && file.bytes.contains(&format!("image: {IMAGE}"))
    }));
    let strict_error = super::render_all(&owned, Some(ExecutionMode::ScaleSet))
        .expect_err("Scale Set routing rejects the native check");
    assert!(
        strict_error
            .to_string()
            .contains("unsupported_darwin_workload:check-maintenance-helpers:native-scale-set"),
        "strict policy error: {strict_error}"
    );
}

fn isolated_generation_root(source: &Path) -> tempfile::TempDir {
    let temporary = tempfile::tempdir().expect("temporary generation root");
    let output = fixture_git_command()
        .args(["ls-files", "-z"])
        .current_dir(source)
        .output()
        .expect("tracked file list is available");
    assert!(output.status.success(), "tracked file listing succeeds");
    copy_tracked_files(source, temporary.path(), &output.stdout);
    initialize_fixture_git(source, temporary.path());
    write_fixture_generator_lock(temporary.path());
    temporary
}

fn initialize_fixture_git(source: &Path, root: &Path) {
    let remote = fixture_git_command()
        .args(["remote", "get-url", "origin"])
        .current_dir(source)
        .output()
        .expect("source origin is readable");
    assert!(remote.status.success(), "source origin is readable");
    let remote = String::from_utf8(remote.stdout)
        .expect("source origin is UTF-8")
        .trim()
        .to_owned();
    assert!(!remote.is_empty(), "source origin is not empty");

    run_git(root, ["init", "--quiet"]);
    run_git(root, ["remote", "add", "origin", remote.as_str()]);
    run_git(root, ["add", "--all"]);
    assert!(
        root.join(".git").is_dir(),
        "fixture uses clone-style Git metadata"
    );
}

fn run_git<const N: usize>(root: &Path, args: [&str; N]) {
    let output = fixture_git_command()
        .args(args)
        .current_dir(root)
        .output()
        .expect("fixture Git command starts");
    assert!(
        output.status.success(),
        "fixture Git command succeeds: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture_git_command() -> Command {
    let mut command = Command::new("git");
    for (name, _) in
        std::env::vars_os().filter(|(name, _)| name.to_string_lossy().starts_with("GIT_"))
    {
        command.env_remove(name);
    }
    command
}

fn copy_tracked_files(source: &Path, destination: &Path, listing: &[u8]) {
    for path in listing
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = PathBuf::from(String::from_utf8_lossy(path).into_owned());
        let source_file = source.join(&relative);
        if !source_file.is_file() {
            continue;
        }
        let destination_file = destination.join(relative);
        fs::create_dir_all(
            destination_file
                .parent()
                .expect("tracked file has a parent"),
        )
        .expect("tracked parent directory is created");
        fs::copy(source_file, destination_file).expect("tracked source file is copied");
    }
}

fn write_fixture_generator_lock(root: &Path) {
    let version = env!("CARGO_PKG_VERSION");
    let mut text = format!(
        "schema = 1\n\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\ncommit = \"{}\"\n",
        "a".repeat(40)
    );
    for target in ReleaseTarget::ALL {
        writeln!(
            text,
            "\n[[generator.binaries]]\ntarget = \"{}\"\nartifact = \"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{}\"\nsha256 = \"{}\"",
            target.triple(),
            target.triple(),
            "b".repeat(64)
        )
        .expect("generator lock fixture formats");
    }
    writeln!(
        text,
        "\n[mise-bootstrap]\nversion = \"{}\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"",
        velnor_actions_mise::MISE_VERSION,
        "c".repeat(64)
    )
    .expect("Mise fixture formats");
    let lock_path = root.join(".velnor/generator.lock");
    fs::write(&lock_path, text).expect("test lock is written to the isolated root");
}
