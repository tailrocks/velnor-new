//! HD-7: a repository with `[private_sources]` generates a tree that passes
//! the pinned jactionlint validation, so every referenced `vars.*` is declared.

use std::fs;

use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::{TestResult, config_with_branch, git, make_repo};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn private_sources_generate_passes_jactionlint_and_declares_the_app_variable() -> TestResult {
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
    let lint_config = fs::read_to_string(preview.join(".github/jactionlint.yaml"))?;
    assert!(lint_config.contains("VELNOR_DEPS_APP_ID"), "{lint_config}");
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
    "Verify restored Rust toolchain",
    "Prepare pinned tools",
    "Prepare Rust components",
    "Verify Rust channel manifest",
    "Restore Cargo sources",
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
    fs::create_dir_all(repo.path().join("nested/src"))?;
    fs::write(
        repo.path().join("nested/Cargo.toml"),
        "[package]\nname = 'nested'\nversion = '0.1.0'\nedition = '2021'\n",
    )?;
    fs::write(repo.path().join("nested/src/lib.rs"), "pub fn f() {}\n")?;
    fs::copy(
        repo.path().join("mise.toml"),
        repo.path().join("nested/mise.toml"),
    )?;
    // Seed grouping follows the locked workspace roots used for source preparation.
    for (root, name) in [("", "demo"), ("nested", "nested")] {
        fs::write(
            repo.path().join(root).join("Cargo.lock"),
            format!("version = 4\n[[package]]\nname = '{name}'\nversion = '0.1.0'\n"),
        )?;
    }
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
    for id in ["dependency-seed", "dependency-seed-nested"] {
        assert_private_seed(&workflow, id)?;
    }
    Ok(())
}

fn assert_private_seed(workflow: &str, id: &str) -> TestResult {
    let seed = workflow
        .split(&format!("\n  {id}:\n"))
        .nth(1)
        .ok_or_else(|| format!("missing private-source seed {id}"))?;
    let seed = seed
        .lines()
        .take_while(|line| line.is_empty() || line.starts_with("    "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(seed.contains("continue-on-error: true"), "{id}: {seed}");
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
    let expected: Vec<_> = SEED_STEPS
        .iter()
        .map(|name| {
            if *name == "Fetch Cargo sources" && id != "dependency-seed" {
                "Fetch Cargo sources (nested/Cargo.toml)"
            } else {
                *name
            }
        })
        .collect();
    // The Plan job is the only elected V2 tools-cache writer; seed jobs
    // restore the shared tool key and only save their MBX bundle.
    assert_eq!(names, expected);
    Ok(())
}
