//! Actual CLI capacity proof for a schema-2 paired `ToFu` workflow.

use std::error::Error;
use std::path::Path;

use crate::impl_cli_tmp::{
    cleanup, code, fresh_tempdir, git_init, install_consumer_manifest, spawn,
};

const MAX_WORKFLOW_BYTES: usize = 500_000;

/// Build a schema-2 ToFu-only repo using both hosted and scale-set lanes.
///
/// Default policy is `ConsumerV1`, so generation requires the release manifest.
fn paired_tofu_repo(roots: usize) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let repo = fresh_tempdir(&format!("tofu-paired-{roots}"))?;
    git_init(&repo)?;
    std::fs::create_dir_all(repo.join(".velnor"))?;
    install_consumer_manifest(&repo)?;
    let names = (0..roots)
        .map(|index| format!("stacks/r{index:03}"))
        .collect::<Vec<_>>();
    let roots = names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        repo.join(".velnor/config.toml"),
        format!(
            "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"main\"\n\
             [execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\n\
             scale_set_profile = \"local\"\nmode = \"both\"\n\
             [execution.profiles.hosted]\nkind = \"github-hosted\"\n\
             label = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
             [execution.profiles.local]\nkind = \"github-scale-set\"\n\
             name = \"ubuntu-26.04-scale-set\"\n\
             labels = [\"ubuntu-26.04-scale-set\", \"velnor\"]\n\
             platform = \"linux/amd64\"\n[stacks.tofu]\nroots = [{roots}]\n"
        ),
    )?;
    for name in names {
        let root = repo.join(&name);
        std::fs::create_dir_all(&root)?;
        let leaf = root
            .file_name()
            .ok_or("ToFu root has no final component")?
            .to_string_lossy();
        std::fs::write(root.join("main.tf"), format!("variable \"{leaf}\" {{}}\n"))?;
    }
    std::fs::write(repo.join("README.md"), "# capacity fixture\n")?;
    Ok(repo)
}

fn workflow_path(preview: &Path) -> std::path::PathBuf {
    preview.join(".github/workflows/ci.yml")
}

#[test]
fn cli_schema2_paired_tofu_stays_within_capacity_and_fails_closed() -> Result<(), Box<dyn Error>> {
    let outer = fresh_tempdir("tofu-paired-output")?;
    let repo = paired_tofu_repo(60)?;
    let preview = outer.join("preview-60");
    let generated = spawn(
        &[
            "generate",
            "--output-dir",
            preview.to_str().ok_or("preview path")?,
        ],
        &[],
        &repo,
    )?;
    assert_eq!(code(&generated), 0, "stderr: {:?}", generated.stderr);
    let workflow = std::fs::read(workflow_path(&preview))?;
    assert!(
        workflow.len() <= MAX_WORKFLOW_BYTES,
        "60-root schema-2 workflow generated {} bytes, limit is {MAX_WORKFLOW_BYTES}",
        workflow.len()
    );
    eprintln!(
        "perf: op=cli-generate schema=2 mode=both roots=60 ci_bytes={}",
        workflow.len()
    );

    let wide_repo = paired_tofu_repo(140)?;
    let wide_preview = outer.join("preview-140");
    let rejected = spawn(
        &[
            "generate",
            "--output-dir",
            wide_preview.to_str().ok_or("preview path")?,
        ],
        &[],
        &wide_repo,
    )?;
    assert_eq!(code(&rejected), 1, "oversized CLI generation must fail");
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    let detail = stderr
        .split("workflow_too_large:.github/workflows/ci.yml:")
        .nth(1)
        .ok_or_else(|| format!("missing workflow size error: {stderr}"))?;
    let actual = detail
        .split_once(':')
        .ok_or_else(|| format!("malformed workflow size error: {detail}"))?
        .0
        .parse::<usize>()?;
    assert!(actual > MAX_WORKFLOW_BYTES, "reported {actual} bytes");
    assert!(
        !workflow_path(&wide_preview).exists(),
        "failed CLI generation left a workflow artifact"
    );
    assert!(
        !wide_preview.exists(),
        "failed CLI generation left a preview tree"
    );
    eprintln!(
        "perf: op=cli-generate schema=2 mode=both roots=140 failed_closed_bytes={actual} limit_bytes={MAX_WORKFLOW_BYTES}"
    );
    cleanup(&repo);
    cleanup(&wide_repo);
    cleanup(&outer);
    Ok(())
}
