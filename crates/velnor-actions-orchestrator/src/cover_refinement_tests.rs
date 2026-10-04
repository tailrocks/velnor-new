//! Qualified broad hints never weaken semantic-input or proof gates.

use super::super::cover_identity_fixtures::*;
use super::super::*;
use super::*;
use velnor_actions_mise::ToolCatalog;

const TASK: &str = "stack/rust/root/clippy/default";

struct Fixture {
    root: tempfile::TempDir,
    discovery: Discovery,
    catalog: ToolCatalog,
    manifest: BaselineManifest,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("root");
        let member = root.path().join("member");
        std::fs::create_dir(&member).expect("member");
        seed_sources(&member);
        std::fs::write(root.path().join("unconsumed.data"), "before").expect("input");
        let mut discovery = discovery_with(&[TASK]);
        discovery.proposals[0].identity.unit_path = "member/Cargo.toml".to_owned();
        let catalog = ToolCatalog::pinned();
        let mut fixture = Self {
            root,
            discovery,
            catalog,
            manifest: manifest_with(&[]),
        };
        fixture.rebind();
        fixture
    }

    fn root_task(kind: &str) -> Self {
        let mut fixture = Self::new();
        let root = fixture.root.path();
        std::fs::rename(root.join("member/Cargo.toml"), root.join("Cargo.toml"))
            .expect("root manifest");
        std::fs::rename(root.join("member/src"), root.join("src")).expect("root sources");
        std::fs::remove_dir(root.join("member")).expect("empty member");
        std::fs::write(root.join("README.md"), "before").expect("readme");
        let task = &mut fixture.discovery.proposals[0];
        task.task_id = format!("stack/rust/root/{kind}/default");
        task.task_kind = kind.to_owned();
        task.identity.unit_path = "Cargo.toml".to_owned();
        fixture.rebind();
        fixture
    }

    fn rebind(&mut self) {
        let task = &self.discovery.proposals[0];
        let live = live_closure_digest(
            self.root.path(),
            &self.discovery,
            &task.task_id,
            &self.catalog,
        );
        self.manifest = manifest_with(&[(&task.task_id, &live)]);
        let snapshot = ExecutionSnapshot::build(&self.discovery).with_checkout(self.root.path());
        let bundle = extension_bundle_with_snapshot(
            &snapshot,
            &self.discovery,
            task,
            Some(self.root.path()),
            None,
        );
        let digest = velnor_actions_contract::digest_b3(b"digest");
        self.manifest.tasks[0].proof = Some(
            ManifestTaskProof::new(
                &task.task_id,
                &digest,
                &digest,
                bundle.graph_digest(),
                &toolchain_id_for_runner(task, &self.catalog, "ubuntu-26.04").expect("toolchain"),
                &live_mbx_digest(task, &self.catalog),
                &platform_id_for_group("ubuntu-26.04", task).expect("platform"),
                &task.configuration,
                7,
            )
            .expect("proof"),
        );
    }

    fn changed(&self) -> ChangedSelection {
        ChangedSelection {
            affected: ["demo".to_owned()].into(),
            proof_refinable: ["demo".to_owned()].into(),
        }
    }

    fn cover(&self, changed: Option<&ChangedSelection>) -> (u32, Plan) {
        let mut plan = plan_with(&[&self.discovery.proposals[0].task_id]);
        let covered = apply_coverage(
            &mut plan,
            &self.manifest,
            &provenance_for(&self.manifest),
            &self.discovery,
            changed,
            &inputs(self.root.path(), &self.catalog),
        );
        (covered, plan)
    }
}

#[test]
fn unconsumed_arbitrary_input_refines_only_with_exact_proof() {
    let fixture = Fixture::new();
    std::fs::write(fixture.root.path().join("unconsumed.data"), "after").expect("edit");
    let changed = fixture.changed();
    let (covered, plan) = fixture.cover(Some(&changed));
    assert_eq!(covered, 1, "{:?}", plan.warnings);
    assert_eq!(
        plan.obligations[0].decision,
        ObligationDecision::CoveredByTrustedBaseline
    );
    assert_eq!(fixture.cover(None).0, 0, "unknown selection executes");
    let conservative = selection(&["demo"]);
    assert_eq!(fixture.cover(Some(&conservative)).0, 0);
}

#[test]
fn missing_or_drifted_structured_proof_refuses_refinement() {
    let mut fixture = Fixture::new();
    let changed = fixture.changed();
    fixture.manifest.tasks[0].proof = None;
    assert_eq!(fixture.cover(Some(&changed)).0, 0);
    let mut fixture = Fixture::new();
    let proof = fixture.manifest.tasks[0].proof.as_ref().expect("proof");
    fixture.manifest.tasks[0].proof = Some(
        ManifestTaskProof::new(
            TASK,
            proof.task_digest(),
            proof.input_digest(),
            &velnor_actions_contract::digest_b3(b"wrong graph"),
            proof.toolchain_id(),
            proof.mbx_digest(),
            proof.platform_id(),
            proof.profile(),
            proof.proof_run_id(),
        )
        .expect("well-formed drift"),
    );
    let (covered, plan) = fixture.cover(Some(&changed));
    assert_eq!(covered, 0);
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.contains("proof_graph_mismatch"))
    );
}

#[test]
fn native_declared_and_configuration_inputs_cannot_refine_edits() {
    for path in [
        "member/native.data",
        "member/schema.proto",
        ".cargo/config.toml",
    ] {
        let mut fixture = Fixture::new();
        let full = fixture.root.path().join(path);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("parent");
        std::fs::write(&full, "before").expect("seed");
        if path.ends_with("schema.proto") {
            fixture.discovery.proposals[0]
                .identity
                .declared_inputs
                .push(path.to_owned());
        }
        fixture.manifest.tasks[0].closure_digest = live_closure_digest(
            fixture.root.path(),
            &fixture.discovery,
            TASK,
            &fixture.catalog,
        );
        std::fs::write(&full, "after").expect("edit");
        let (covered, plan) = fixture.cover(Some(&fixture.changed()));
        assert_eq!(covered, 0, "{path}: {:?}", plan.warnings);
    }
}

#[test]
fn included_readme_build_script_and_unknown_inputs_refuse() {
    for source in [
        "pub const README: &str = include_str!(\"../README.md\");",
        "pub fn f() { std::fs::read(\"data\"); }",
    ] {
        let fixture = Fixture::new();
        std::fs::write(fixture.root.path().join("member/README.md"), "docs").expect("docs");
        std::fs::write(fixture.root.path().join("member/src/lib.rs"), source).expect("source");
        let (covered, plan) = fixture.cover(Some(&fixture.changed()));
        assert_eq!(covered, 0);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("incomplete_inputs"))
        );
    }
    let fixture = Fixture::new();
    std::fs::write(fixture.root.path().join("member/build.rs"), "fn main() {}")
        .expect("build script");
    assert_eq!(fixture.cover(Some(&fixture.changed())).0, 0);
    let mut fixture = Fixture::new();
    fixture.discovery.proposals[0]
        .identity
        .declared_inputs
        .push("missing.proto".to_owned());
    assert_eq!(fixture.cover(Some(&fixture.changed())).0, 0);
}

#[test]
fn source_edit_stays_executing_inside_unowned_broadening() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.path().join("member/src/lib.rs"),
        "pub fn changed() {}",
    )
    .expect("source");
    std::fs::write(fixture.root.path().join("unconsumed.data"), "after").expect("unowned");
    assert_eq!(fixture.cover(Some(&fixture.changed())).0, 0);
}

#[test]
fn empty_unit_requires_every_same_manifest_rust_owner_refinable() {
    let fixture = Fixture::new();
    let mut empty = fixture.discovery.proposals[0].clone();
    empty.identity.unit_id.clear();
    let mut definite = fixture.discovery.proposals[0].clone();
    definite.identity.unit_id = "definite".to_owned();
    let changed = fixture.changed();
    let entry = &fixture.manifest.tasks[0];
    assert!(permits(
        &empty,
        Some(&changed),
        &[&fixture.discovery.proposals[0]],
        entry
    ));
    assert!(!permits(
        &empty,
        Some(&changed),
        &[&fixture.discovery.proposals[0], &definite],
        entry
    ));
    assert!(!permits(&empty, Some(&changed), &[], entry));
    for stack in ["tofu", "workload", "unknown"] {
        let mut task = fixture.discovery.proposals[0].clone();
        task.stack_id = stack.to_owned();
        assert!(!permits(&task, Some(&changed), &[&task], entry));
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_inventory_never_refines() {
    use std::os::unix::ffi::OsStringExt;
    let fixture = Fixture::new();
    let name = std::ffi::OsString::from_vec(vec![b'u', 0xff]);
    std::fs::write(fixture.root.path().join(name), "unknown").expect("non UTF8 input");
    assert_eq!(fixture.cover(Some(&fixture.changed())).0, 0);
}

#[path = "cover_refinement_root_tests.rs"]
mod root_tests;
