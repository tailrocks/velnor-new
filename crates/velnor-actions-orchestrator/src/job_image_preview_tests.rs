//! Check source identity and output boundaries for the job image candidate.

use std::{fs, path::Path};

use tempfile::TempDir;
use velnor_actions_contract::ExecutionMode;

use super::preview_job_image_candidate;

const CONFIG: &str = r#"schema = 2
[workflow]
policy = "velnor-repository-v1"
[execution]
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
workflows = ["image_release"]
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
"#;

fn fixture(config: &str, origin: &str) -> TempDir {
    let directory = tempfile::tempdir().expect("Create the test directory.");
    let root = directory.path();
    fs::create_dir(root.join(".velnor")).expect("Create the configuration directory.");
    fs::write(root.join(".velnor/config.toml"), config).expect("Write the configuration.");
    let initialized = crate::git_fixture::sterile_command()
        .args(["init", "--quiet"])
        .arg(root)
        .status()
        .expect("Initialize the test repository.");
    assert!(initialized.success());
    let configured = crate::git_fixture::command(root)
        .expect("Read the test repository.")
        .args(["remote", "add", "origin", origin])
        .status()
        .expect("Set the source origin.");
    assert!(configured.success());
    directory
}

fn candidate(root: &Path) -> std::path::PathBuf {
    root.join(".github/workflows/job-image-candidate.yml")
}

#[test]
fn candidate_preview_repeats_without_task_discovery_or_a_job_image_pin() {
    let source = fixture(CONFIG, "https://github.com/tailrocks/velnor-new.git");
    let destination = tempfile::tempdir().expect("Create the output parent.");
    let first = destination.path().join("first");
    let second = destination.path().join("second");
    let paths =
        preview_job_image_candidate(source.path(), &first, None).expect("Generate the workflow.");
    preview_job_image_candidate(source.path(), &second, None)
        .expect("Generate the workflow again.");
    assert_eq!(paths, vec![".github/workflows/job-image-candidate.yml"]);
    assert_eq!(
        fs::read(candidate(&first)).expect("Read the first workflow."),
        fs::read(candidate(&second)).expect("Read the second workflow.")
    );
    assert!(!source.path().join(".github").exists());
    assert!(!source.path().join("Cargo.toml").exists());
    let output = fs::read_to_string(candidate(&first)).expect("Read the workflow text.");
    assert!(output.contains("candidate/candidate.py build"));
    assert!(output.contains("runs-on: ubuntu-26.04"));
}

#[test]
fn candidate_preview_rejects_consumer_policy_and_wrong_origin_before_output() {
    for (config, origin) in [
        (
            CONFIG.replace("velnor-repository-v1", "consumer-v1"),
            "https://github.com/tailrocks/velnor-new.git",
        ),
        (CONFIG.to_owned(), "https://github.com/example/consumer.git"),
    ] {
        let source = fixture(&config, origin);
        let output = tempfile::tempdir().expect("Create the output directory.");
        let destination = output.path().join("candidate");
        assert!(preview_job_image_candidate(source.path(), &destination, None).is_err());
        assert!(!destination.exists());
    }
}

#[test]
fn candidate_preview_requires_image_release_before_output() {
    let config = CONFIG.replace("[\"image_release\"]", "[]");
    let source = fixture(&config, "https://github.com/tailrocks/velnor-new.git");
    let output = tempfile::tempdir().expect("Create the output directory.");
    let destination = output.path().join("candidate");
    assert!(preview_job_image_candidate(source.path(), &destination, None).is_err());
    assert!(!destination.exists());
}

#[test]
fn candidate_preview_routes_containerless_scale_set_and_writes_only_candidate() {
    let source = fixture(CONFIG, "https://github.com/tailrocks/velnor-new.git");
    let output = tempfile::tempdir().expect("Create the output directory.");
    let destination = output.path().join("candidate");
    let paths =
        preview_job_image_candidate(source.path(), &destination, Some(ExecutionMode::ScaleSet))
            .expect("Generate the containerless Scale Set workflow.");
    assert_eq!(paths, vec![".github/workflows/job-image-candidate.yml"]);
    let rendered = fs::read_to_string(candidate(&destination)).expect("Read the workflow.");
    assert_eq!(
        rendered
            .matches("runs-on: [velnor, ubuntu-26.04-scale-set]")
            .count(),
        3
    );
    assert!(!rendered.contains("container:"));
    assert!(!source.path().join(".github").exists());
    assert!(!source.path().join("Cargo.toml").exists());
}

#[test]
fn candidate_preview_rejects_hosted_label_in_scale_set_before_output() {
    let config = CONFIG.replace(
        "labels = [\"velnor\", \"ubuntu-26.04-scale-set\"]",
        "labels = [\"velnor\", \"ubuntu-26.04\"]",
    );
    let source = fixture(&config, "https://github.com/tailrocks/velnor-new.git");
    let output = tempfile::tempdir().expect("Create the output directory.");
    let destination = output.path().join("candidate");
    assert!(
        preview_job_image_candidate(source.path(), &destination, Some(ExecutionMode::ScaleSet))
            .is_err()
    );
    assert!(!destination.exists());
}

#[test]
fn candidate_preview_rejects_a_destination_inside_the_source() {
    let source = fixture(CONFIG, "https://github.com/tailrocks/velnor-new.git");
    let destination = source.path().join("candidate");
    assert!(preview_job_image_candidate(source.path(), &destination, None).is_err());
    assert!(!destination.exists());
}
