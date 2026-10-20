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
