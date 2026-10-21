//! HD-7: a repository with `[private_sources]` generates a tree that passes
//! the pinned actionlint validation, so every referenced `vars.*` is declared.

use std::fs;

use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::{TestResult, config_with_branch, git, make_repo};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn private_sources_generate_passes_actionlint_and_declares_the_app_variable() -> TestResult {
    let config = format!(
        "{}[private_sources]\nowner = \"acme\"\napp_id_variable = \"VELNOR_DEPS_APP_ID\"\n\
app_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\n\
[[private_sources.release_assets]]\nid = \"eth-sql\"\nrepository = \"acme/eth-sql\"\n\
tag = \"v1.0.0\"\nasset = \"eth-sql.tar.gz\"\nextract_to = \"eth-sql\"\n",
        config_with_branch()
    );
    let repo = make_repo(&config)?;
    fs::write(
        repo.path().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(
        repo.path().join(".velnor/deps.lock"),
        format!(
            "schema = 1\n[[release_assets]]\nid = \"eth-sql\"\nrepository = \"acme/eth-sql\"\n\
tag = \"v1.0.0\"\ncommit = \"{COMMIT}\"\nasset = \"eth-sql.tar.gz\"\nsha256 = \"{SHA}\"\n\
extract_to = \"eth-sql\"\n"
        ),
    )?;
    git(&["add", "."], repo.path())?;
    let prep = prepare(repo.path())?;
    let out = tempfile::TempDir::new()?;
    let preview = out.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    let workflow = fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
    assert!(workflow.contains("vars.VELNOR_DEPS_APP_ID"), "{workflow}");
    let mint = workflow
        .find("Mint read-only token for private sources")
        .expect("The test failed.");
    let fetch = workflow
        .find("Fetch private sources")
        .expect("The test failed.");
    let revoke = workflow
        .find("Revoke private-source token")
        .expect("The test failed.");
    assert!(mint < fetch && fetch < revoke);
    assert!(
        workflow
            .lines()
            .any(|line| line.trim() == "repositories: eth-sql"),
        "{workflow}"
    );
    assert!(workflow.contains("permission-contents: read"), "{workflow}");
    assert!(
        workflow.contains("secrets.VELNOR_DEPS_APP_PRIVATE_KEY"),
        "{workflow}"
    );
    let actionlint = fs::read_to_string(preview.join(".github/actionlint.yaml"))?;
    assert!(actionlint.contains("VELNOR_DEPS_APP_ID"), "{actionlint}");
    Ok(())
}

fn private_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let config = format!(
        "{}[private_sources]\nowner = \"acme\"\napp_id_variable = \"VELNOR_DEPS_APP_ID\"\n\
app_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\n\
[[private_sources.release_assets]]\nid = \"eth-sql\"\nrepository = \"acme/eth-sql\"\n\
tag = \"v1.0.0\"\nasset = \"eth-sql.tar.gz\"\nextract_to = \"eth-sql\"\n",
        config_with_branch()
    );
    let repo = make_repo(&config)?;
    fs::write(
        repo.path().join(".velnor/deps.lock"),
        format!(
            "schema = 1\n[[release_assets]]\nid = \"eth-sql\"\nrepository = \"acme/eth-sql\"\n\
tag = \"v1.0.0\"\ncommit = \"{COMMIT}\"\nasset = \"eth-sql.tar.gz\"\nsha256 = \"{SHA}\"\n\
extract_to = \"eth-sql\"\n"
        ),
    )?;
    Ok(repo)
}

/// Golden step order of the mbx + private-sources seed job.
const SEED_STEPS: &[&str] = &[
    "Record optional cache budget",
    "Checkout",
    "Acquire Velnor",
    "Install cache tar adapters",
    "Setup Mise",
    "Resolve project tool versions",
    "V2 identity",
    "Restore Mise tools",
    "Prepare pinned tools",
    "Prepare Rust components",
    "Verify Rust channel manifest",
    "Verify Rust before MBX action",
    "Set up MBX",
    "Verify native MBX version",
    "Compute MBX bundle key",
    "Restore MBX bundle",
    "Import MBX bundle",
    "Mint read-only token for private sources",
    "Fetch private sources",
    "Revoke private-source token",
    "Fetch Cargo sources",
    "Compile test executables (seed)",
    "Compile Clippy lane (seed)",
    "Compile documentation lane (seed)",
    "Record MBX producer success",
    "Export MBX bundle",
    "Admit optional cache save",
    "Save MBX bundle",
    "Report optional cache save failure",
    "Clean MBX workspace outputs",
];

/// In v0.1.37, `mr_boxington = true` with a private release asset failed with `internal_without_acquire:dependency-seed`.
/// The seed job ran the private fetch helper before staging it.
#[test]
fn mbx_seed_job_with_private_sources_stages_the_helper_before_the_fetch() -> TestResult {
    let repo = private_repo()?;
    fs::write(
        repo.path().join("mise.toml"),
        format!(
            "[tools]\nrust = {{ version = \"{}\", mr_boxington = true }}\nmr-boxington = \"{}\"\n",
            velnor_actions_mise::catalog::RUST_VERSION,
            velnor_actions_mise::catalog::MR_BOXINGTON_VERSION
        ),
    )?;
    git(&["add", "."], repo.path())?;
    let prep = prepare(repo.path())?;
    assert_eq!(
        prep.discovery.workspaces[0].profile.compile_driver.as_str(),
        "mbx"
    );
    let out = tempfile::TempDir::new()?;
    let preview = out.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    let workflow = fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
    let seed = workflow
        .split("\n  dependency-seed:\n")
        .nth(1)
        .ok_or("The dependency seed job must exist.")?;
    let seed = seed.split("\n  rust-").next().unwrap_or(seed);
    let acquire = seed
        .find("Acquire Velnor")
        .ok_or("The seed job must stage the helper.")?;
    let fetch = seed
        .find("vars.VELNOR_DEPS_APP_ID")
        .ok_or("The seed job must mint the token.")?;
    assert!(
        acquire < fetch,
        "The seed job must acquire the helper before the private fetch.\n{seed}"
    );
    let names: Vec<&str> = seed
        .lines()
        .filter_map(|line| {
            line.trim_start()
                .trim_start_matches("- ")
                .strip_prefix("name: ")
        })
        .skip(1)
        .take_while(|name| *name != "Plan")
        .collect();
    assert_eq!(names, SEED_STEPS);
    Ok(())
}
