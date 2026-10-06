//! Proposal-boundary equality: metadata, extension, closure (H8-H11).
//!
//! The neutral [`ProposedTask`](velnor_actions_contract::ProposedTask)
//! path must equal the group path byte-for-byte: entry metadata,
//! identity extension, and closure inputs. Refactor-only: any drift
//! here is a behavior change.
use crate::support::{Outcome, TempDir};
use velnor_actions_contract::ProposedTask;
use velnor_actions_rust::tasks::{DigestSlot, RustTaskIdentityExtension, TaskGroup, TaskKind};
use velnor_actions_rust::{
    CompileDriver, Evidence, EvidenceStrength, GroupExtensionInputs, NextestProfile, TestRunner,
    adapter_entry_metadata, entry_metadata_for_task, extension_for_proposal, propose_task,
    resolve_closure_at_root,
};

/// Fully-loaded group exercising every converted field.
fn group() -> TaskGroup {
    TaskGroup {
        task_id: "stack/rust/crates-a/clippy/default".to_owned(),
        package_id: "path+file:///repo#demo@0.1.0".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "crates/a".to_owned(),
        kind: TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: vec!["a".to_owned()],
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: Some("demo".to_owned()),
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: vec!["proto/a.proto".to_owned()],
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}

/// Extension inputs over one proposal's precomputed identity facts.
fn inputs<'a>(task: &'a ProposedTask, targets: &'a [String]) -> GroupExtensionInputs<'a> {
    GroupExtensionInputs {
        package_id: &task.identity.unit_id,
        workspace_id: "workspace",
        profile: "default",
        manifest: &task.identity.unit_path,
        graph_digest: "graph",
        targets,
        config_digest: "config",
        lock_digest: DigestSlot::Known("lock".to_owned()),
        nextest_digest: DigestSlot::Known("nextest".to_owned()),
        archive_source: None,
        rerun_inputs: None,
        has_build_script: false,
    }
}

/// Serialize one extension for byte-equality comparisons.
fn json_of(ext: &RustTaskIdentityExtension) -> Option<serde_json::Value> {
    serde_json::to_value(ext).ok()
}

/// Group inputs mirroring [`inputs`] over the group fields.
fn group_inputs<'a>(
    group: &'a TaskGroup,
    manifest: &'a str,
    targets: &'a [String],
) -> GroupExtensionInputs<'a> {
    GroupExtensionInputs {
        package_id: &group.package_id,
        workspace_id: "workspace",
        profile: "default",
        manifest,
        graph_digest: "graph",
        targets,
        config_digest: "config",
        lock_digest: DigestSlot::Known("lock".to_owned()),
        nextest_digest: DigestSlot::Known("nextest".to_owned()),
        archive_source: None,
        rerun_inputs: None,
        has_build_script: false,
    }
}

#[test]
fn metadata_proposal_matches_group() {
    let group = group();
    let task = propose_task(&group).expect("valid group proposes");
    let evidence = vec![
        Evidence {
            path: ".config/nextest.toml".to_owned(),
            line: 2,
            command_or_setting: "profile.ci".to_owned(),
            strength: EvidenceStrength::Durable,
        },
        Evidence {
            path: ".github/workflows/ci.yml".to_owned(),
            line: 9,
            command_or_setting: "cargo nextest run".to_owned(),
            strength: EvidenceStrength::Transient,
        },
    ];
    assert_eq!(
        entry_metadata_for_task(&task, &evidence).expect("proposal metadata"),
        adapter_entry_metadata(&group, &evidence),
    );
    assert_eq!(
        entry_metadata_for_task(&task, &[]).expect("empty evidence"),
        adapter_entry_metadata(&group, &[]),
    );
}

#[test]
fn extension_proposal_matches_group() {
    let group = group();
    let task = propose_task(&group).expect("valid group proposes");
    let targets = vec!["lib".to_owned()];
    let from_group =
        group.identity_extension(&group_inputs(&group, "crates/a/Cargo.toml", &targets));
    let from_task =
        extension_for_proposal(&task, &inputs(&task, &targets)).expect("proposal extension");
    assert_eq!(json_of(&from_task), json_of(&from_group));
}

#[test]
fn extension_proposal_matches_group_nextest() {
    let mut group = group();
    group.kind = TaskKind::Nextest;
    group.task_id = "stack/rust/crates-a/nextest/default".to_owned();
    group.test_runner = TestRunner::CargoNextest;
    group.target_flags = vec!["--tests".to_owned()];
    let task = propose_task(&group).expect("valid group proposes");
    let targets = vec!["lib".to_owned(), "bins".to_owned()];
    let from_group =
        group.identity_extension(&group_inputs(&group, "crates/a/Cargo.toml", &targets));
    let from_task =
        extension_for_proposal(&task, &inputs(&task, &targets)).expect("proposal extension");
    assert_eq!(json_of(&from_task), json_of(&from_group));
}

#[test]
fn closure_input_names_pinned() -> Outcome {
    let dir = TempDir::create("propose-closure")?;
    dir.write("crates/a/Cargo.toml", "[package]\nname = \"a\"\n")?;
    dir.write("crates/a/src/lib.rs", "fn a() {}\n")?;
    dir.write("proto/a.proto", "syntax = \"proto3\";\n")?;
    let group = group();
    let task = propose_task(&group).expect("valid group proposes");
    let closure = resolve_closure_at_root(
        dir.path(),
        &task,
        None,
        "graph",
        "toolchain",
        "platform",
        &velnor_actions_rust::semantic_inputs::SemanticInventory {
            paths: vec![
                "crates/a/Cargo.toml".to_owned(),
                "crates/a/src/lib.rs".to_owned(),
                "proto/a.proto".to_owned(),
            ],
            provenance: velnor_actions_contract::Provenance::Known {
                digest: "checkout".to_owned(),
            },
        },
    )?;
    assert_eq!(closure.task_id, task.task_id);
    let names: Vec<&str> = closure.inputs.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        vec![
            "cargo_config",
            "declared_extra:0:proto/a.proto",
            "driver",
            "features",
            "kind",
            "local_deps",
            "lockfile",
            "manifest",
            "nextest_config",
            "platform",
            "profile",
            "runner",
            "source_tree",
            "target",
            "toolchain",
            "vcs",
        ]
    );
    assert!(closure.verify_complete().is_ok());
    Ok(())
}

#[test]
fn closure_vcs_unknown_with_undeclared_reads() -> Outcome {
    let dir = TempDir::create("propose-closure-vcs")?;
    dir.write("crates/a/Cargo.toml", "[package]\nname = \"a\"\n")?;
    dir.write("crates/a/src/lib.rs", "fn a() {}\n")?;
    dir.write("proto/a.proto", "syntax = \"proto3\";\n")?;
    let mut group = group();
    group.undeclared_reads = true;
    let task = propose_task(&group).expect("valid group proposes");
    let closure = resolve_closure_at_root(
        dir.path(),
        &task,
        None,
        "graph",
        "toolchain",
        "platform",
        &velnor_actions_rust::semantic_inputs::SemanticInventory {
            paths: vec![
                "crates/a/Cargo.toml".to_owned(),
                "crates/a/src/lib.rs".to_owned(),
                "proto/a.proto".to_owned(),
            ],
            provenance: velnor_actions_contract::Provenance::Known {
                digest: "checkout".to_owned(),
            },
        },
    )?;
    assert_eq!(closure.unknown_inputs(), vec!["vcs"]);
    assert!(closure.verify_complete().is_err());
    Ok(())
}

/// Complete plain local packages permit narrow source proof.
fn semantic_fixture() -> Result<
    (
        TempDir,
        velnor_actions_rust::semantic_inputs::SemanticInventory,
    ),
    Box<dyn std::error::Error>,
> {
    let dir = TempDir::create("semantic-local")?;
    let files = [
        (
            "Cargo.toml",
            "[workspace]\nmembers = ['crates/a', 'crates/b', 'crates/c']\n",
        ),
        (
            "crates/a/Cargo.toml",
            "[package]\nname = 'a'\nversion = '0.1.0'\n[dependencies]\nb = { path = '../b' }\n",
        ),
        ("crates/a/src/lib.rs", "pub fn a() -> u8 { 1 }\n"),
        ("crates/a/README.md", "unconsumed documentation\n"),
        (
            "crates/b/Cargo.toml",
            "[package]\nname = 'b'\nversion = '0.1.0'\n",
        ),
        ("crates/b/src/lib.rs", "pub fn b() -> u8 { 1 }\n"),
        (
            "crates/c/Cargo.toml",
            "[package]\nname = 'c'\nversion = '0.1.0'\n",
        ),
        ("crates/c/src/lib.rs", "pub fn c() -> u8 { 1 }\n"),
        ("proto/a.proto", "syntax = 'proto3';\n"),
    ];
    for (path, text) in files {
        dir.write(path, text)?;
    }
    let inventory = velnor_actions_rust::semantic_inputs::SemanticInventory {
        paths: files.iter().map(|(path, _)| (*path).to_owned()).collect(),
        provenance: velnor_actions_contract::Provenance::Known {
            digest: "inventory".to_owned(),
        },
    };
    Ok((dir, inventory))
}

#[test]
fn semantic_dependency_bytes_change_proof_but_unconsumed_docs_and_members_do_not() -> Outcome {
    let (dir, inventory) = semantic_fixture()?;
    let task = propose_task(&group())?;
    let resolve = || {
        resolve_closure_at_root(
            dir.path(),
            &task,
            None,
            "graph",
            "tools",
            "platform",
            &inventory,
        )
    };
    let before = resolve()?;
    before.verify_complete()?;
    let bytes = serde_json::to_vec(&before)?;
    dir.write("crates/a/README.md", "updated unconsumed docs\n")?;
    dir.write("crates/c/src/lib.rs", "pub fn c() -> u8 { 2 }\n")?;
    assert_eq!(serde_json::to_vec(&resolve()?)?, bytes);
    dir.write("crates/b/src/lib.rs", "pub fn b() -> u8 { 2 }\n")?;
    assert_ne!(serde_json::to_vec(&resolve()?)?, bytes);
    Ok(())
}

#[test]
fn included_docs_and_native_build_inputs_are_unknown_until_complete_proof() -> Outcome {
    let (dir, inventory) = semantic_fixture()?;
    let task = propose_task(&group())?;
    dir.write(
        "crates/a/src/lib.rs",
        "pub const DOC: &str = include_str!(\"../README.md\");\n",
    )?;
    let docs = resolve_closure_at_root(
        dir.path(),
        &task,
        None,
        "graph",
        "tools",
        "platform",
        &inventory,
    )?;
    assert!(docs.verify_complete().is_err());
    assert!(docs.unknown_inputs().contains(&"source_tree"));
    dir.write("crates/a/src/lib.rs", "pub fn a() -> u8 { 1 }\n")?;
    dir.write(
        "crates/a/build.rs",
        "fn main() { cc::Build::new().file(\"native.c\").compile(\"native\"); }\n",
    )?;
    dir.write("crates/a/native.c", "int a(void) { return 1; }\n")?;
    let native = resolve_closure_at_root(
        dir.path(),
        &task,
        None,
        "graph",
        "tools",
        "platform",
        &inventory,
    )?;
    assert!(native.verify_complete().is_err());
    Ok(())
}
