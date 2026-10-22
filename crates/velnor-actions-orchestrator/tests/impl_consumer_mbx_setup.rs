//! Test the standard consumer Rust setup with MBX and mirrored Rust toolchain declarations.
//! Only the dependency seed job writes MBX bundles.
//!
//! The fixture uses the owner's standard layout.
//! Its `mise.toml` declares `min_version` and `rust = { version, mr_boxington = true }`.
//! It pins exact versions of `mr-boxington`, hk, cargo-binstall, and Nextest.
//! It sets binstall, idiomatic version file, and lock file options.
//! It also declares `[wrappers.cargo] command = "mbx"`.
//! Its `rust-toolchain.toml` mirrors the version, components, and targets.
//! Its `.velnor/config.toml` has no `[stacks.rust] compile_driver` declaration.

use std::fs;

use velnor_actions_mise::catalog::{MR_BOXINGTON_VERSION, NEXTEST_VERSION, RUST_VERSION};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use crate::impl_common::{TestResult, config_with_branch, make_repo, write_nextest_task};

/// The owner's standard consumer `mise.toml` (docs/implemented/consumer-rust-setup.md).
fn consumer_mise_toml() -> String {
    format!(
        "min_version = \"2026.10.17\"\n\n[tools]\nrust = {{ version = \"{RUST_VERSION}\", profile = \"minimal\", components = [\"clippy\", \"rustfmt\"], targets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"], mr_boxington = true }}\nmr-boxington = \"{MR_BOXINGTON_VERSION}\"\nhk = \"2.8.0\"\ncargo-binstall = \"1.26.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"{NEXTEST_VERSION}\"\n\n[settings]\ncargo.binstall = true\nidiomatic_version_file_enable_tools = [\"rust\"]\nlockfile = true\n\n[wrappers.cargo]\ncommand = \"mbx\"\nenv = {{ MBX_CARGO_SHIM_MODE = \"1\" }}\n"
    )
}

/// The owner's standard `rust-toolchain.toml`, mirroring the Mise version.
fn consumer_toolchain() -> String {
    format!(
        "[toolchain]\nchannel = \"{RUST_VERSION}\"\nprofile = \"minimal\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n"
    )
}

/// Write the owner layout into a fixture repository.
fn write_layout(root: &std::path::Path) -> TestResult {
    fs::write(root.join("mise.toml"), consumer_mise_toml())?;
    fs::write(root.join("rust-toolchain.toml"), consumer_toolchain())?;
    write_nextest_task(root)
}

/// Split rendered `ci.yml` into `(job id, job text)` blocks.
fn jobs(yaml: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_jobs = false;
    for line in yaml.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        if let Some(id) = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' '))
            .and_then(|rest| rest.strip_suffix(':'))
        {
            out.push((id.to_owned(), String::new()));
        } else if let Some((_, text)) = out.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    out
}

fn step_count(job: &str, name: &str) -> usize {
    job.lines()
        .filter(|line| line.trim_start().trim_start_matches("- ") == format!("name: {name}"))
        .count()
}

#[test]
fn mr_boxington_option_selects_mbx_seed_writer_and_opaque_bundles() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_layout(root)?;
    let prep = prepare(root)?;

    assert_eq!(prep.discovery.workspaces.len(), 1);
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(
        workspace.profile.compile_driver.as_str(),
        "mbx",
        "The mr_boxington = true setting must select the MBX driver."
    );
    assert!(
        workspace.findings.is_empty(),
        "The configuration must not produce a transient-evidence finding: {:?}.",
        workspace.findings
    );
    assert!(prep.catalog.rust_mr_boxington());

    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("The CI workflow must exist.")?;
    assert!(
        !yaml.contains("github-cache-mode"),
        "The MBX action must only install MBX. It must not cache the target directory: {yaml}."
    );
    let jobs = jobs(yaml);
    let seed: Vec<_> = jobs
        .iter()
        .filter(|(_, job)| job.contains("name: Dependency seed"))
        .collect();
    assert_eq!(
        seed.len(),
        1,
        "The workflow must contain one dependency seed job: {yaml}."
    );
    let (seed_id, seed_job) = seed[0];
    for name in [
        "Compute MBX bundle key",
        "Restore MBX bundle",
        "Import MBX bundle",
        "Record MBX producer success",
        "Export MBX bundle",
        "Save MBX bundle",
    ] {
        assert_eq!(step_count(seed_job, name), 1, "{seed_id} runs {name}");
    }
    let bundle_helper = tree
        .get(".github/scripts/velnor-mbx-bundle.sh")
        .ok_or("The MBX bundle helper must be rendered.")?;
    assert!(
        bundle_helper
            .contains("cache export --group \"$MBX_CACHE_EXPORT_GROUP\" --format directory"),
        "The helper must export the MBX bundle: {bundle_helper}."
    );
    assert!(
        bundle_helper.contains("cache import \"$dir\""),
        "The helper must import restored MBX bundles: {bundle_helper}."
    );

    let crate_jobs: Vec<_> = jobs
        .iter()
        .filter(|(id, _)| id.starts_with("rust-"))
        .collect();
    assert!(
        !crate_jobs.is_empty(),
        "The workflow must contain a crate job: {yaml}."
    );
    for (id, job) in crate_jobs {
        assert_eq!(step_count(job, "Restore MBX bundle"), 1, "{id} restores");
        assert_eq!(step_count(job, "Import MBX bundle"), 1, "{id} imports");
        for writer in ["Export MBX bundle", "Save MBX bundle"] {
            assert_eq!(
                step_count(job, writer),
                0,
                "Job {id} must only read bundles. Only the seed job may write them."
            );
        }
    }
    Ok(())
}

#[test]
fn mr_boxington_consumer_generates_without_a_declared_driver() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_layout(root)?;
    let prep = prepare(root)?;
    let parent = tempfile::TempDir::new()?;
    let out = parent.path().join("out");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(out.clone()),
        },
    )?;
    let ci = fs::read_to_string(out.join(".github/workflows/ci.yml"))?;
    assert!(ci.contains("Import MBX bundle"), "{ci}");
    Ok(())
}

#[test]
fn owner_layout_with_the_mise_authority_check_and_private_sources_generates() -> TestResult {
    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let config = format!(
        "{}[stacks.rust]\ntoolchain_source = \"mise\"\n[private_sources]\nowner = \"acme\"\n\
app_id_variable = \"VELNOR_DEPS_APP_ID\"\napp_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\n\
[[private_sources.release_assets]]\nid = \"eth-sql\"\nrepository = \"acme/eth-sql\"\n\
tag = \"v1.0.0\"\nasset = \"eth-sql.tar.gz\"\nextract_to = \"eth-sql\"\n",
        config_with_branch()
    );
    let repo = make_repo(&config)?;
    let root = repo.path();
    write_layout(root)?;
    fs::write(
        root.join(".velnor/deps.lock"),
        format!(
            "schema = 1\n[[release_assets]]\nid = \"eth-sql\"\nrepository = \"acme/eth-sql\"\n\
tag = \"v1.0.0\"\ncommit = \"{COMMIT}\"\nasset = \"eth-sql.tar.gz\"\nsha256 = \"{SHA}\"\n\
extract_to = \"eth-sql\"\n"
        ),
    )?;
    crate::impl_common::git(&["add", "."], root)?;
    let prep = prepare(root)?;
    assert_eq!(
        prep.discovery.workspaces[0].profile.compile_driver.as_str(),
        "mbx"
    );
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("The CI workflow must exist.")?;
    assert!(yaml.contains("name: Dependency seed"), "{yaml}");
    assert!(yaml.contains("Fetch private sources"), "{yaml}");

    // Generation fails if rust-toolchain.toml omits a target from the Mise declaration.
    fs::write(
        root.join("rust-toolchain.toml"),
        consumer_toolchain().replace(", \"x86_64-unknown-linux-gnu\"", ""),
    )?;
    let err = prepare(root)
        .err()
        .ok_or("A target mismatch must fail generation.")?
        .to_string();
    assert!(
        err.contains("rust_toolchain_mirror_mismatch:targets:"),
        "{err}"
    );

    // Generation fails if rust-toolchain.toml and mise.toml declare different versions.
    fs::write(
        root.join("rust-toolchain.toml"),
        consumer_toolchain().replace(RUST_VERSION, "1.98.1"),
    )?;
    let err = prepare(root)
        .err()
        .ok_or("A version mismatch must fail generation.")?
        .to_string();
    assert!(
        err.contains("rust_version_disagrees_with_mise:rust-toolchain.toml=1.98.1"),
        "{err}"
    );
    Ok(())
}
