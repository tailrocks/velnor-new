//! Root-directory ownership remains a hint; adapter closure is the proof.

use super::*;

#[test]
fn root_readme_clippy_covers_but_doc_and_doctest_execute() {
    for kind in ["clippy", "doc", "doctest"] {
        let fixture = Fixture::root_task(kind);
        std::fs::write(fixture.root.path().join("README.md"), "after").expect("readme edit");
        let (covered, plan) = fixture.cover(Some(&fixture.changed()));
        assert_eq!(
            covered,
            u32::from(kind == "clippy"),
            "{kind}: {:?}",
            plan.warnings
        );
        if kind != "clippy" {
            assert!(
                plan.warnings
                    .iter()
                    .any(|warning| warning.contains("closure_mismatch"))
            );
        }
    }
}

#[test]
fn root_source_or_nonmarkdown_bytes_with_readme_edit_execute() {
    for path in ["src/lib.rs", "unconsumed.data"] {
        let fixture = Fixture::root_task("clippy");
        std::fs::write(fixture.root.path().join("README.md"), "after").expect("readme edit");
        std::fs::write(fixture.root.path().join(path), "pub fn changed() {}")
            .expect("consumed input");
        let (covered, plan) = fixture.cover(Some(&fixture.changed()));
        assert_eq!(covered, 0, "{path}: {:?}", plan.warnings);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("closure_mismatch"))
        );
    }
}

#[test]
fn root_readme_cannot_cover_opaque_macro_runtime_or_build_inputs() {
    for source in [
        "pub const README: &str = include_str!(\"../README.md\");",
        "pub fn f() { std::fs::read(\"README.md\"); }",
    ] {
        let fixture = Fixture::root_task("clippy");
        std::fs::write(fixture.root.path().join("src/lib.rs"), source).expect("source");
        std::fs::write(fixture.root.path().join("README.md"), "after").expect("readme");
        let (covered, plan) = fixture.cover(Some(&fixture.changed()));
        assert_eq!(covered, 0);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("incomplete_inputs"))
        );
    }
    for opaque in ["build", "proc-macro"] {
        let fixture = Fixture::root_task("clippy");
        if opaque == "build" {
            std::fs::write(fixture.root.path().join("build.rs"), "fn main() {}")
                .expect("build script");
        } else {
            std::fs::write(
                fixture.root.path().join("Cargo.toml"),
                "[package]\nname = \"demo\"\n[lib]\nproc-macro = true\n",
            )
            .expect("proc macro manifest");
        }
        std::fs::write(fixture.root.path().join("README.md"), "after").expect("readme");
        let (covered, plan) = fixture.cover(Some(&fixture.changed()));
        assert_eq!(covered, 0);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("incomplete_inputs"))
        );
    }
}

#[test]
fn root_readme_absent_proof_or_stale_provenance_never_covers() {
    let mut fixture = Fixture::root_task("clippy");
    std::fs::write(fixture.root.path().join("README.md"), "after").expect("readme");
    fixture.manifest.tasks[0].proof = None;
    assert_eq!(fixture.cover(Some(&fixture.changed())).0, 0);
    for expired in [false, true] {
        let mut fixture = Fixture::root_task("clippy");
        std::fs::write(fixture.root.path().join("README.md"), "after").expect("readme");
        let changed = fixture.changed();
        let mut plan = plan_with(&[&fixture.discovery.proposals[0].task_id]);
        plan.base = Some("a".repeat(40));
        if expired {
            fixture.manifest.expires_at_unix = Some(1);
        } else {
            fixture.manifest.generator_sha256 = "2".repeat(64);
        }
        let mut baseline_inputs = inputs(fixture.root.path(), &fixture.catalog);
        baseline_inputs.repository = Some("o/r");
        crate::cover_baseline::apply_baseline(
            &mut plan,
            velnor_actions_contract::WorkflowEvent::PullRequest,
            baseline_inputs,
            Some(fixture.manifest),
            &fixture.discovery,
            Some(&changed),
        )
        .expect("baseline classification");
        assert_eq!(plan.obligations[0].decision, ObligationDecision::Execute);
        assert!(plan.baseline.reason().is_some_and(|reason| if expired {
            reason == "baseline_expired"
        } else {
            reason.starts_with("baseline_invalid:")
        }));
    }
}
