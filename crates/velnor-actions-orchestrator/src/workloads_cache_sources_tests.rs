//! Generated metadata agreement does not imply source publicity.

use super::*;
use velnor_actions_contract::config::WorkloadConfig;

fn source(name: &str) -> NativeNpmSource {
    let basename = name.rsplit('/').next().expect("package basename");
    NativeNpmSource {
        name: name.to_owned(),
        version: "1.2.3".to_owned(),
        resolved: format!("https://registry.npmjs.org/{name}/-/{basename}-1.2.3.tgz"),
        integrity: format!("sha512-{}==", "A".repeat(86)),
    }
}

fn task(kind: &str, sources: Option<&[NativeNpmSource]>) -> ProposedTask {
    let workload: WorkloadConfig = serde_json::from_value(serde_json::json!({
        "name": "app", "kind": kind, "root": "docs"
    }))
    .expect("closed workload");
    let mut task = super::super::proposal(&workload, "install", Vec::new(), None);
    if let Some(sources) = sources {
        let key = if kind == "bun_ci" { BUN_KEY } else { NPM_KEY };
        task.identity.environment.insert(
            key.to_owned(),
            serde_json::to_string(sources).expect("descriptor JSON"),
        );
    }
    task
}

#[test]
fn absent_metadata_disables_transport_without_removing_obligations() {
    let install = task("node_ci", None);
    let mut build = install.clone();
    build.task_kind = "build".to_owned();
    let tasks = [&install, &build];
    assert!(native_sources_for_tasks(&tasks).expect("absent").is_empty());
    assert_eq!(tasks.len(), 2);
    assert!(native_sources_for_tasks(&[]).expect("empty").is_empty());
}

#[test]
fn source_tuples_are_canonical_and_private_scopes_remain_candidates() {
    let private = source("@private/secret");
    let public = source("public-package");
    let install = task("node_ci", Some(&[public.clone(), private.clone()]));
    let build = task(
        "node_ci",
        Some(&[private.clone(), public.clone(), private.clone()]),
    );
    assert_eq!(
        native_sources_for_tasks(&[&install, &build]).expect("candidate agreement"),
        [private, public]
    );
}

#[test]
fn every_obligation_must_carry_identical_candidate_evidence() {
    let install = task("node_ci", Some(&[source("a")]));
    let absent = task("node_ci", None);
    let different = task("node_ci", Some(&[source("b")]));
    assert!(native_sources_for_tasks(&[&install, &absent]).is_err());
    assert!(native_sources_for_tasks(&[&absent, &install]).is_err());
    assert!(native_sources_for_tasks(&[&install, &different]).is_err());
}

#[test]
fn malformed_and_invalid_candidate_metadata_is_rejected() {
    for value in ["not JSON", "{}", "[{}]", "[null]"] {
        let mut task = task("node_ci", None);
        task.identity
            .environment
            .insert(NPM_KEY.to_owned(), value.to_owned());
        assert!(native_sources_for_tasks(&[&task]).is_err(), "{value}");
    }
    let mut invalid = source("a");
    invalid.resolved = "https://registry.npmjs.org/b/-/b-1.2.3.tgz".to_owned();
    assert!(native_sources_for_tasks(&[&task("node_ci", Some(&[invalid]))]).is_err());
    let mut unknown = serde_json::to_value(source("a")).expect("source");
    unknown["public"] = true.into();
    let mut task = task("node_ci", None);
    task.identity
        .environment
        .insert(NPM_KEY.to_owned(), format!("[{unknown}]"));
    assert!(native_sources_for_tasks(&[&task]).is_err());
}

#[test]
fn mixed_kinds_roots_and_wrong_markers_are_rejected() {
    let node = task("node_ci", Some(&[source("a")]));
    let bun = task("bun_ci", Some(&[source("a")]));
    assert!(native_sources_for_tasks(&[&node, &bun]).is_err());
    let mut root = node.clone();
    root.identity.project_root = "other".to_owned();
    assert!(native_sources_for_tasks(&[&node, &root]).is_err());
    let mut wrong = node.clone();
    wrong.configuration = "ruby_syntax".to_owned();
    assert!(native_sources_for_tasks(&[&wrong]).is_err());
    let mut both = node;
    both.identity
        .environment
        .insert(BUN_KEY.to_owned(), "[]".to_owned());
    assert!(native_sources_for_tasks(&[&both]).is_err());
    assert_eq!(
        native_sources_for_tasks(&[&bun]).expect("Bun evidence"),
        [source("a")]
    );
}

#[test]
fn descriptors_are_bounded_before_decoding_or_dispatch() {
    let mut oversized = task("node_ci", None);
    oversized
        .identity
        .environment
        .insert(NPM_KEY.to_owned(), " ".repeat(MAX_DESCRIPTOR_BYTES + 1));
    assert!(native_sources_for_tasks(&[&oversized]).is_err());
    let excessive = task("node_ci", Some(&vec![source("a"); MAX_SOURCES + 1]));
    assert!(native_sources_for_tasks(&[&excessive]).is_err());
}
