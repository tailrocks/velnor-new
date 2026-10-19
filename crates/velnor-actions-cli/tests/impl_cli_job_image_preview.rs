//! Check job image workflow generation through the CLI.

use std::{error::Error, fs};

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, git_fixture, git_init, spawn_isolated};

#[test]
fn candidate_preview_cli_requires_output_and_rejects_conflicting_previews()
-> Result<(), Box<dyn Error>> {
    let root = fresh_tempdir("image-preview-arguments")?;
    for args in [
        vec!["generate", "--job-image-candidate-only"],
        vec![
            "generate",
            "--job-image-candidate-only",
            "--owned-tool-candidates-only",
            "--output-dir",
            "/tmp/unused-image-preview",
        ],
    ] {
        let result = spawn_isolated(&args, &[], &root)?;
        assert_eq!(code(&result), 2);
    }
    cleanup(&root);
    Ok(())
}

#[test]
fn candidate_preview_cli_writes_only_the_candidate_workflow() -> Result<(), Box<dyn Error>> {
    let root = fresh_tempdir("image-preview-source")?;
    git_init(&root)?;
    let origin = git_fixture::command(&root)?
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ])
        .output()?;
    assert!(origin.status.success());
    fs::create_dir(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), CONFIG)?;
    let output = fresh_tempdir("image-preview-output")?;
    let destination = output.join("candidate");
    let result = spawn_isolated(
        &[
            "generate",
            "--job-image-candidate-only",
            "--output-dir",
            destination
                .to_str()
                .ok_or("The output path is not UTF-8.")?,
        ],
        &[],
        &root,
    )?;
    assert_eq!(
        code(&result),
        0,
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        result.stdout,
        b".github/workflows/job-image-candidate.yml\n"
    );
    assert!(!root.join(".github").exists());
    let workflow =
        fs::read_to_string(destination.join(".github/workflows/job-image-candidate.yml"))?;
    assert!(workflow.contains("candidate/candidate.py build"));
    assert!(workflow.contains("runs-on: ubuntu-26.04"));
    cleanup(&root);
    cleanup(&output);
    Ok(())
}

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
