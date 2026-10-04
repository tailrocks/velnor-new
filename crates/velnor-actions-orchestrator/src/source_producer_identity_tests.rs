//! Source identity projects compiler pins out; runtime authority retains them.

use velnor_actions_contract::{CompiledSourceHelper, HelperInvocation, SourceBoundHelper};

use super::{MODE, RustSourceDescriptor, RustSourceSelection, source_identity};

fn fixture() -> RustSourceDescriptor {
    let checksum = "a".repeat(64);
    let descriptor = RustSourceDescriptor {
        schema: 1,
        rust_version: velnor_actions_mise::catalog::RUST_VERSION.to_owned(),
        target: velnor_actions_mise::catalog::RUST_TARGET_TRIPLE.to_owned(),
        roots: vec![String::new()],
        manifests: vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname='demo'\nversion='1.0.0'\n[dependencies]\nz='1'\n".to_owned(),
        )],
        locks: vec![(
            String::new(),
            format!(
                "version=4\n[[package]]\nname='demo'\nversion='1.0.0'\n\
                 [[package]]\nname='z'\nversion='1.0.0'\n\
                 source='registry+https://github.com/rust-lang/crates.io-index'\n\
                 checksum='{checksum}'\n"
            ),
        )],
        archives: vec![("z".to_owned(), "1.0.0".to_owned(), checksum)],
        mode: MODE.to_owned(),
        selections: Vec::new(),
    };
    RustSourceDescriptor::from_hex(&descriptor.hex_json().expect("canonical wire"))
        .expect("current public locked descriptor")
}

fn helper(descriptor: &RustSourceDescriptor) -> CompiledSourceHelper {
    super::super::source::compiled_helper(descriptor, env!("CARGO_PKG_VERSION"))
        .expect("actual compiled acquisition owner")
}

fn key(descriptor: &RustSourceDescriptor) -> String {
    source_identity(descriptor, &helper(descriptor)).expect("source owner identity")
}

#[test]
fn compiler_pin_changes_runtime_wire_but_preserves_actual_source_owner_key() {
    let current = fixture();
    let mut other_compiler = current.clone();
    other_compiler.rust_version = "1.97.0".to_owned();
    let current_helper = helper(&current);
    let other_helper = helper(&other_compiler);
    assert_eq!(current_helper.source(), other_helper.source());
    assert_eq!(
        current_helper.invocation().descriptor(),
        other_helper.invocation().descriptor()
    );
    assert_ne!(
        current_helper.invocation().args(),
        other_helper.invocation().args()
    );
    assert_ne!(
        current.json().expect("JSON"),
        other_compiler.json().expect("JSON")
    );
    assert_eq!(
        source_identity(&current, &current_helper).expect("current key"),
        source_identity(&other_compiler, &other_helper).expect("other compiler key")
    );
    assert_eq!(
        super::super::transport::Source3::new(key(&current)).expect("current transport"),
        super::super::transport::Source3::new(key(&other_compiler)).expect("other transport")
    );
    assert!(RustSourceDescriptor::from_hex(&current.hex_json().expect("current wire")).is_ok());
    assert!(
        RustSourceDescriptor::from_hex(&other_compiler.hex_json().expect("other wire")).is_err()
    );
}

#[test]
fn source_projection_retains_every_noncompiler_runtime_field() {
    let original = fixture();
    let baseline = key(&original);
    let mut mutations = Vec::new();
    let mut changed = original.clone();
    changed.schema += 1;
    mutations.push(("schema", changed));
    let mut changed = original.clone();
    changed.target = "aarch64-unknown-linux-gnu".to_owned();
    mutations.push(("target", changed));
    let mut changed = original.clone();
    changed.roots.push("nested".to_owned());
    mutations.push(("roots", changed));
    let mut changed = original.clone();
    changed.manifests[0]
        .1
        .push_str("# changed captured manifest\n");
    mutations.push(("manifests", changed));
    let mut changed = original.clone();
    changed.locks[0].1.push_str("# changed captured lock\n");
    mutations.push(("locks", changed));
    let mut changed = original.clone();
    changed.archives[0].2 = "b".repeat(64);
    mutations.push(("archives", changed));
    let mut changed = original.clone();
    changed.mode = "native-tree-selected-containing".to_owned();
    mutations.push(("mode", changed));
    let mut changed = original.clone();
    changed.selections = vec![selection()];
    mutations.push(("selections", changed));
    for (field, changed) in mutations {
        assert_eq!(helper(&original).source(), helper(&changed).source());
        assert_ne!(baseline, key(&changed), "projection lost {field}");
    }
}

fn selection() -> RustSourceSelection {
    RustSourceSelection {
        root: String::new(),
        package: "demo".to_owned(),
        target: None,
        features: Vec::new(),
        default_features: true,
    }
}

#[test]
fn selected_target_features_and_defaults_remain_source_identity_inputs() {
    let mut original = fixture();
    original.mode = "native-tree-selected-containing".to_owned();
    original.selections = vec![selection()];
    let original = RustSourceDescriptor::from_hex(&original.hex_json().expect("selected wire"))
        .expect("qualified selected descriptor");
    let baseline = key(&original);
    let mut target = original.clone();
    target.selections[0].target = Some("aarch64-apple-darwin".to_owned());
    let mut defaults = original.clone();
    defaults.selections[0].default_features = false;
    let mut features = defaults.clone();
    features.selections[0].features = vec!["optional".to_owned()];
    for changed in [&target, &defaults, &features] {
        RustSourceDescriptor::from_hex(&changed.hex_json().expect("selected wire"))
            .expect("qualified changed selection");
        assert_ne!(baseline, key(changed));
    }
    assert_ne!(key(&defaults), key(&features));
}

#[test]
fn qualified_locked_archive_revision_changes_actual_source_key() {
    let original = fixture();
    let mut changed = original.clone();
    changed.locks[0].1 = changed.locks[0]
        .1
        .replace("name='z'\nversion='1.0.0'", "name='z'\nversion='1.0.1'")
        .replace(&"a".repeat(64), &"b".repeat(64));
    changed.archives[0].1 = "1.0.1".to_owned();
    changed.archives[0].2 = "b".repeat(64);
    let changed = RustSourceDescriptor::from_hex(&changed.hex_json().expect("revision wire"))
        .expect("qualified new locked archive");
    assert_eq!(original.manifests, changed.manifests);
    assert_eq!(original.target, changed.target);
    assert_ne!(original.locks, changed.locks);
    assert_ne!(original.archives, changed.archives);
    assert_eq!(helper(&original).source(), helper(&changed).source());
    assert_ne!(key(&original), key(&changed));
}

#[test]
fn acquisition_program_digest_and_selection_package_or_root_affect_identity() {
    let original = fixture();
    let original_helper = helper(&original);
    let changed_source = format!("{}# acquisition revision\n", original_helper.source());
    let operation = original_helper.invocation().descriptor().operation();
    let owner = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &velnor_actions_contract::compiled_source_sha256(changed_source.as_bytes()),
    )
    .expect("changed owner descriptor");
    let invocation = HelperInvocation::compiled(
        owner,
        original_helper.invocation().args().to_vec(),
        Vec::new(),
    )
    .expect("same runtime arguments");
    let revised_helper = CompiledSourceHelper::compiled(invocation, changed_source)
        .expect("self-consistent program revision");
    assert_ne!(
        source_identity(&original, &original_helper).expect("original key"),
        source_identity(&original, &revised_helper).expect("revised program key")
    );
    let mut selected = original;
    selected.mode = "native-tree-selected-containing".to_owned();
    selected.selections = vec![selection()];
    let mut package = selected.clone();
    package.selections[0].package = "different".to_owned();
    let mut root = selected.clone();
    root.selections[0].root = "nested".to_owned();
    assert_ne!(key(&selected), key(&package));
    assert_ne!(key(&selected), key(&root));
}
