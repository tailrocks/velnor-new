//! Baseline entry tests.
//!
//! Declared via `#[path]` from `cover_baseline.rs` under `cfg(test)`.

use super::*;

#[test]
fn baseline_publish_and_download_rules() {
    assert!(publish_event_eligible(WorkflowEvent::Push));
    assert!(!publish_event_eligible(WorkflowEvent::PullRequest));
    assert!(!publish_event_eligible(WorkflowEvent::MergeGroup));
    let base = "a".repeat(40);
    let dir = Path::new("/tmp/x");
    let name = format!("velnor-baseline-{base}-{}", digest_b3(b"c"));
    let named: Vec<String> = baseline_download_args(
        &base,
        ".github/workflows/ci.yml",
        "testmain",
        Some(&name),
        7,
        dir,
        "o/r",
    )
    .iter()
    .map(|arg| arg.to_string_lossy().into_owned())
    .collect();
    assert_eq!(&named[0..4], &["run", "download", "7", "--name"]);
    assert_eq!(named[4], name);
    assert_eq!(&named[named.len() - 2..], &["--repo", "o/r"]);
    assert!(baseline_download_args(&base, "w", "b", None, 7, dir, "o/r").is_empty());
    assert!(baseline_download_args(&base, "w", "b", Some(""), 7, dir, "o/r").is_empty());
    assert!(
        baseline_download_args(&base, "w", "b", Some(&name), 7, dir, "not-a-slug").is_empty(),
        "a malformed repo yields no unscoped command"
    );
}

/// Minimal valid manifest JSON for `base`/`name`, run 7 attempt 1.
fn manifest_json(base: &str, name: &str) -> serde_json::Value {
    let digest = digest_b3(b"d");
    let numeric = crate::cover_compat::baseline_artifact_numeric_id(name);
    serde_json::json!({
        "schema": 2,
        "repository_id": digest,
        "source_commit": base,
        "ref": "refs/heads/testmain",
        "event": "push",
        "workflow_ref": "o/r/.github/workflows/ci.yml@refs/heads/testmain",
        "run_id": 7,
        "run_attempt": 1,
        "final_status": "passed",
        "generator_version": "0.1.0",
        "generator_sha256": "1".repeat(64),
        "compatibility_id": digest,
        "artifact_id": numeric,
        "artifact_name": name,
        "tasks": [],
    })
}

#[test]
fn baseline_entry_needs_single_strict_payload() {
    let base = "a".repeat(40);
    let name = format!("velnor-baseline-{base}-{}", digest_b3(b"c"));
    let numeric = crate::cover_compat::baseline_artifact_numeric_id(&name);
    let tmp = tempfile::tempdir().expect("tempdir");
    let entry = tmp.path().join(&name);
    std::fs::create_dir(&entry).expect("entry");
    std::fs::write(entry.join("baseline.json"), "{}").expect("json");
    std::fs::write(entry.join("extra.json"), "{}").expect("extra");
    assert!(baseline_entry_for(&entry, &base, 7, 1, numeric).is_none());
    std::fs::remove_file(entry.join("extra.json")).expect("rm");
    assert!(baseline_entry_for(&entry, &base, 7, 1, numeric).is_none());
    std::fs::write(entry.join("baseline.json"), r#"{"schema": 1, "schema": 1}"#).expect("dup");
    assert!(baseline_entry_for(&entry, &base, 7, 1, numeric).is_none());
    std::fs::write(entry.join("baseline.json"), [0xff, 0xfe]).expect("bad");
    assert!(baseline_entry_for(&entry, &base, 7, 1, numeric).is_none());
    let manifest = manifest_json(&base, &name);
    std::fs::write(entry.join("baseline.json"), manifest.to_string()).expect("manifest");
    let found = baseline_entry_for(&entry, &base, 7, 1, numeric).expect("entry");
    assert_eq!(found.artifact_name, name);
    assert!(
        baseline_entry_for(&entry, &base, 8, 1, numeric).is_none(),
        "a manifest claiming another run never loads from this download"
    );
    let mut stale = manifest;
    stale["schema"] = serde_json::json!(1);
    std::fs::write(entry.join("baseline.json"), stale.to_string()).expect("stale");
    assert!(
        baseline_entry_for(&entry, &base, 7, 1, numeric).is_none(),
        "schema 1 baselines bound no source bytes and never load"
    );
}

/// Plan with one execute obligation and a marker generator SHA.
fn marker_plan(marker: &str) -> velnor_actions_contract::Plan {
    use velnor_actions_contract::{
        ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation,
        PlanRunner, RunnerSelection, Trust,
    };
    let digest = digest_b3(b"d");
    Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: "plan-local".to_owned(),
        base: Some("a".repeat(40)),
        head: "head".to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: marker.to_owned(),
        },
        packages: Vec::new(),
        obligations: vec![PlanObligation {
            task_id: "stack/rust/root/clippy/default".to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest: digest.clone(),
            input_digest: digest.clone(),
            closure_digest: digest,
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: vec!["stack/rust/root/clippy/default".to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

/// Discovery without task groups.
fn empty_discovery() -> crate::discover::Discovery {
    crate::discover::Discovery {
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        consumer_manifest_stand_in: false,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

/// Repository slug the test checkout must anchor: the CI-provided
/// `GITHUB_REPOSITORY` when well-formed, else the `o/r` fixture slug.
/// Keeps anchor fixtures green whether or not CI env is present.
fn anchor_slug() -> String {
    let shaped = |slug: &str| {
        let mut parts = slug.split('/');
        matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(owner), Some(repo), None)
                if !owner.is_empty()
                    && !repo.is_empty()
                    && !slug.chars().any(char::is_whitespace)
        )
    };
    std::env::var("GITHUB_REPOSITORY")
        .ok()
        .filter(|slug| shaped(slug))
        .map_or_else(|| "o/r".to_owned(), |slug| slug.to_lowercase())
}

/// Minimal git checkout anchoring `slug` as its origin.
fn anchored_checkout(slug: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let git = tmp.path().join(".git");
    std::fs::create_dir_all(git.join("objects")).expect("objects");
    std::fs::create_dir_all(git.join("refs")).expect("refs");
    std::fs::write(git.join("HEAD"), "ref: refs/heads/testmain\n").expect("HEAD");
    std::fs::write(
        git.join("config"),
        format!("[remote \"origin\"]\n\turl = https://github.com/{slug}.git\n"),
    )
    .expect("config");
    tmp
}

/// Valid manifest over `slug`/`base` except one forwarded proof run.
fn forwarded_manifest(slug: &str, base: &str) -> BaselineManifest {
    let digest = digest_b3(b"d");
    let name = super::provenance_check::baseline_artifact_name(base, &digest).expect("name");
    BaselineManifest {
        schema: 2,
        repository_id: digest_b3(format!("github.com/{slug}").as_bytes()),
        source_commit: base.to_owned(),
        ref_: "refs/heads/testmain".to_owned(),
        event: "push".to_owned(),
        workflow_ref: format!("{slug}/.github/workflows/ci.yml@refs/heads/testmain"),
        run_id: 7,
        run_attempt: 1,
        final_status: "passed".to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        generator_sha256: "1".repeat(64),
        compatibility_id: digest.clone(),
        artifact_id: crate::cover_compat::baseline_artifact_numeric_id(&name),
        artifact_name: name,
        tasks: vec![crate::merge::required_evidence::BaselineTaskEntry {
            task_id: "stack/rust/root/clippy/default".to_owned(),
            task_digest: digest.clone(),
            input_digest: digest.clone(),
            closure_digest: digest,
            proof_run_id: 5,
            observed_run_id: 7,
            external_data: None,
            proof: None,
        }],
        expires_at_unix: None,
    }
}

/// Forwarded proofs fail closed at the caller: the plan marks the
/// baseline unavailable with the exact miss token, warns once, and
/// keeps every obligation executing.
#[test]
fn forwarded_proof_marks_baseline_unavailable() {
    use velnor_actions_contract::ObligationDecision;
    use velnor_actions_mise::ToolCatalog;
    let slug = anchor_slug();
    let checkout = anchored_checkout(&slug);
    let base = "a".repeat(40);
    let manifest = forwarded_manifest(&slug, &base);
    let marker = "1".repeat(64);
    let mut plan = marker_plan(&marker);
    let catalog = ToolCatalog::pinned();
    let inputs = BaselineInputs {
        branch: "testmain",
        root: checkout.path(),
        workflow: ".github/workflows/ci.yml",
        catalog: &catalog,
        repository: None,
    };
    apply_baseline(
        &mut plan,
        WorkflowEvent::PullRequest,
        inputs,
        Some(manifest),
        &empty_discovery(),
        None,
    )
    .expect("classify");
    assert_eq!(
        plan.baseline.reason(),
        Some("baseline_invalid:originating_run_unverified")
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w == "baseline_miss:originating_run_unverified"),
        "{:?}",
        plan.warnings
    );
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
}

/// No lock fill: a marker-sha plan keeps its marker through baseline
/// classification even with a pinning lock on disk, and source builds
/// skip live lookup with their reason instead of emitting a pin. A
/// failing validation likewise persists no lock identity.
#[test]
fn source_build_keeps_marker_without_lock_fill() {
    use velnor_actions_mise::ToolCatalog;
    let marker = crate::internal_plan::snapshot::UNRESOLVED_GENERATOR_SHA.to_owned();
    let mut plan = marker_plan(&marker);
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tmp.path().join(".velnor")).expect("dir");
    std::fs::write(
        tmp.path().join(".velnor/generator.lock"),
        format!(
            "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{}\"\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .expect("lock");
    let discovery = empty_discovery();
    let catalog = ToolCatalog::pinned();
    let inputs = BaselineInputs {
        branch: "testmain",
        root: tmp.path(),
        workflow: ".github/workflows/ci.yml",
        catalog: &catalog,
        repository: None,
    };
    apply_baseline(
        &mut plan,
        WorkflowEvent::PullRequest,
        inputs,
        None,
        &discovery,
        None,
    )
    .expect("classify");
    assert_eq!(plan.generator.sha256, marker, "no lock fill");
    assert_eq!(
        plan.baseline.reason(),
        Some(crate::cover_identity::SOURCE_BUILD_REASON)
    );
}

/// Attempt and artifact pins: a claim of attempt 1 never loads when the
/// run succeeded on attempt 3, and a foreign artifact id never loads.
#[test]
fn baseline_entry_pins_attempt_and_artifact() {
    let base = "a".repeat(40);
    let name = format!("velnor-baseline-{base}-{}", digest_b3(b"c"));
    let numeric = crate::cover_compat::baseline_artifact_numeric_id(&name);
    let tmp = tempfile::tempdir().expect("tempdir");
    let entry = tmp.path().join(&name);
    std::fs::create_dir(&entry).expect("entry");
    let manifest = manifest_json(&base, &name);
    std::fs::write(entry.join("baseline.json"), manifest.to_string()).expect("manifest");
    assert!(
        baseline_entry_for(&entry, &base, 7, 3, numeric).is_none(),
        "claims attempt 1, success on 3"
    );
    assert!(
        baseline_entry_for(&entry, &base, 7, 1, numeric.wrapping_add(1)).is_none(),
        "foreign artifact id"
    );
    assert!(baseline_entry_for(&entry, &base, 7, 1, numeric).is_some());
}

/// Symlink, traversal, and size gates: a linked payload, a linked entry
/// dir, a directory payload, and an oversize payload never load, even
/// when every name and claim is otherwise valid.
#[test]
fn baseline_entry_rejects_links_and_oversize() {
    let base = "a".repeat(40);
    let name = format!("velnor-baseline-{base}-{}", digest_b3(b"c"));
    let numeric = crate::cover_compat::baseline_artifact_numeric_id(&name);
    #[cfg(unix)]
    {
        let manifest = manifest_json(&base, &name).to_string();
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("real.json"), &manifest).expect("real");
        let linked = tmp.path().join(&name);
        std::fs::create_dir(&linked).expect("linked");
        std::os::unix::fs::symlink(tmp.path().join("real.json"), linked.join("baseline.json"))
            .expect("link");
        assert!(
            baseline_entry_for(&linked, &base, 7, 1, numeric).is_none(),
            "symlinked payload rejects even at a live target"
        );
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).expect("target");
        std::fs::write(target.join("baseline.json"), &manifest).expect("manifest");
        let via = tmp.path().join(&name);
        std::os::unix::fs::symlink(&target, &via).expect("dir link");
        assert!(
            baseline_entry_for(&via, &base, 7, 1, numeric).is_none(),
            "symlinked entry dir rejects"
        );
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let entry = tmp.path().join(&name);
    std::fs::create_dir(&entry).expect("entry");
    std::fs::create_dir(entry.join("baseline.json")).expect("dir payload");
    assert!(
        baseline_entry_for(&entry, &base, 7, 1, numeric).is_none(),
        "directory payload rejects"
    );
    std::fs::remove_dir(entry.join("baseline.json")).expect("rmdir");
    let big = format!(
        r#"{{"schema":2,"pad":"{}"}}"#,
        "p".repeat(MAX_BASELINE_MANIFEST_BYTES)
    );
    std::fs::write(entry.join("baseline.json"), big).expect("big");
    assert!(
        baseline_entry_for(&entry, &base, 7, 1, numeric).is_none(),
        "oversize payload rejects"
    );
}
