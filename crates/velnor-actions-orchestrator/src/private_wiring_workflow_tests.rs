//! The real workflow boundary: `build_workflow` plus the renderer over a
//! consumer repository with private sources.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_rust::TaskKind;

use super::*;
use crate::crate_jobs::crate_jobs_tests::{discovery, group};

const PRIVATE_LOCK: &str = "[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n[[package]]\nname = \"shared\"\nversion = \"1\"\nsource = \"git+https://github.com/ChainArgos/processor-shared?tag=v1#0123456789abcdef0123456789abcdef01234567\"\n";

/// Schema-only consumer release manifest (debug builds skip the binary check).
fn manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "b".repeat(40)
    )
}

fn render(dir: &tempfile::TempDir, roots: &[String]) -> Result<String, OrchestratorError> {
    let config = load_config(dir.path())?;
    let proposals = vec![
        group("demo", TaskKind::Clippy, &[]),
        group("demo", TaskKind::Build, &[]),
    ];
    let mut found = discovery(proposals);
    found.consumer_manifest_json = Some(manifest_json());
    let workflow = crate::workflow::build_workflow(
        dir.path(),
        &config,
        "main",
        "ubuntu-26.04",
        &found,
        roots,
    )?;
    velnor_actions_workflow_renderer::render::render_workflow_ir(
        &workflow.ir,
        WorkflowPolicy::ConsumerV1,
        workflow.support.as_ref(),
        &workflow.context,
    )
    .map_err(|err| OrchestratorError::Render {
        problem: err.to_string(),
    })
}

/// The text of one job section.
fn section<'a>(yaml: &'a str, id: &str) -> &'a str {
    let header = format!("\n  {id}:\n");
    let start = yaml
        .find(&header)
        .unwrap_or_else(|| panic!("job {id}:\n{yaml}"))
        + 1;
    let rest = &yaml[start..];
    let end = rest
        .match_indices("\n  ")
        .map(|(at, _)| at)
        .find(|at| !rest[at + 3..].starts_with(' '))
        .unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn plan_and_crate_jobs_mint_fetch_and_revoke_before_any_repository_step() {
    let dir = workspace(&format!("{BASE}{TABLE}"), PRIVATE_LOCK);
    let yaml = render(&dir, &[String::new()]).expect("render");
    for id in ["plan", "rust-demo"] {
        let job = section(&yaml, id);
        let at = |needle: &str| {
            job.find(needle)
                .unwrap_or_else(|| panic!("{id}: missing {needle}:\n{job}"))
        };
        let acquire = at("- name: Acquire Velnor");
        let mint = at("- name: Mint read-only token for private sources");
        let fetch = at("- name: Fetch private sources");
        let revoke = at("- name: Revoke private-source token");
        let cargo = at("- name: Fetch Cargo sources");
        assert!(
            acquire < mint && mint < fetch && fetch < revoke && revoke < cargo,
            "{id}:\n{job}"
        );
        for needle in [
            "repositories: processor-shared",
            "VELNOR_PRIVATE_REPOSITORIES: processor-shared",
            "VELNOR_PRIVATE_SOURCES: cargo,release-assets",
            "VELNOR_PRIVATE_FETCH_ROOTS: .",
            "VELNOR_INTERNAL_OP: private-fetch-v1",
            "VELNOR_INTERNAL_OP: private-revoke-v1",
            "if: always() && steps.deps-token.outcome == 'success'",
        ] {
            assert!(job.contains(needle), "{id}: {needle}\n{job}");
        }
        assert_eq!(
            job.matches("steps.deps-token.outputs.token").count(),
            2,
            "{id}: the token is bound in the fetch and revoke steps only"
        );
        assert_eq!(
            job.matches("secrets.VELNOR_DEPS_APP_PRIVATE_KEY").count(),
            1,
            "{id}: the App key is named once, by the mint"
        );
        let first_repository_step = job
            .find("- name: Clippy")
            .or_else(|| job.find("- name: Build test executables"))
            .unwrap_or(job.len());
        assert!(revoke < first_repository_step, "{id}: revoked first");
    }
}

#[test]
fn no_private_closure_or_no_table_means_no_credential_at_all() {
    let plain = "[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n";
    let with_table = workspace(&format!("{BASE}{TABLE}"), plain);
    let yaml = render(&with_table, &[String::new()]).expect("render");
    assert!(
        !yaml.contains("deps-token") && !yaml.contains("VELNOR_DEPS_APP"),
        "{yaml}"
    );
    let without_table = workspace(BASE, PRIVATE_LOCK);
    let yaml = render(&without_table, &[String::new()]).expect("render");
    assert!(!yaml.contains("deps-token"), "no table, no credential");
}

#[test]
fn a_lookalike_git_host_never_earns_a_token() {
    let hostile = PRIVATE_LOCK.replace(
        "github.com/ChainArgos",
        "github.com.evil.example/ChainArgos",
    );
    let dir = workspace(&format!("{BASE}{TABLE}"), &hostile);
    let yaml = render(&dir, &[String::new()]).expect("render");
    assert!(!yaml.contains("deps-token"), "{yaml}");
}

#[test]
fn generation_is_deterministic_and_carries_no_secret_material() {
    let dir = workspace(&format!("{BASE}{TABLE}"), PRIVATE_LOCK);
    let first = render(&dir, &[String::new()]).expect("render");
    assert_eq!(first, render(&dir, &[String::new()]).expect("render"));
    for forbidden in ["ghs_", "BEGIN RSA", "BEGIN PRIVATE"] {
        assert!(!first.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn a_changed_private_source_changes_the_allowlist_literal() {
    let dir = workspace(&format!("{BASE}{TABLE}"), PRIVATE_LOCK);
    let before = render(&dir, &[String::new()]).expect("render");
    fs::write(
        dir.path().join("Cargo.lock"),
        format!("{PRIVATE_LOCK}[[package]]\nname = \"extra\"\nversion = \"1\"\nsource = \"git+https://github.com/ChainArgos/eth-domain#0123456789abcdef0123456789abcdef01234567\"\n"),
    )
    .expect("lock");
    let after = render(&dir, &[String::new()]).expect("render");
    assert_ne!(before, after);
    assert!(
        after.contains("repositories: eth-domain,processor-shared"),
        "{after}"
    );
}
