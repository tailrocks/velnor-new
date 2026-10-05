//! The MBX qualification workflow shares one isolated Mise data dir.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

const QUALIFICATION_MISE_DATA_DIR: &str =
    "${{ github.workspace }}/.velnor-mbx-cache-qualification/mise";

#[test]
fn mbx_qualification_job_and_setup_share_isolated_mise_data_dir() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;

    for job_id in ["mbx-cache-write-hosted", "mbx-cache-read-hosted"] {
        let job = crate::impl_schema2_routing::job_body(qualification, job_id)?;
        let job_fields = job.split_once("    steps:").ok_or("job steps")?.0;
        let job_data_dir = env_value(job_fields, "MISE_DATA_DIR").ok_or("job Mise data dir")?;
        let setup = named_step_body(job, "Set up Mise")?;
        let setup_data_dir = env_value(setup, "MISE_DATA_DIR").ok_or("setup Mise data dir")?;

        assert_eq!(job_data_dir, setup_data_dir, "{job_id}");
        assert_eq!(job_data_dir, QUALIFICATION_MISE_DATA_DIR, "{job_id}");
        assert!(
            !job.contains("/.velnor/cache/mise"),
            "{job_id} stays isolated from the shared Velnor tool cache"
        );
    }
    Ok(())
}

/// Locate one action step and its continuation lines.
fn named_step_body<'a>(job: &'a str, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let marker = format!("      - name: {name}");
    let start = job.find(&marker).ok_or("missing Mise setup step")?;
    let tail = &job[start..];
    let end = tail[marker.len()..]
        .find("\n      - name:")
        .map_or(tail.len(), |offset| marker.len() + offset);
    Ok(&tail[..end])
}

/// Read one rendered `env` key from a bounded YAML scope.
fn env_value<'a>(scope: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}: ");
    scope
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix.as_str()))
}
