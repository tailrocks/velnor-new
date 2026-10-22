use super::super::super::normal_tofu::ValidatedTofuRuntime;
use super::super::super::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use super::super::{PayloadManifest, PayloadRoot};

fn runtime() -> ValidatedTofuRuntime {
    ValidatedTofuRuntime {
        target: "x86_64-unknown-linux-gnu".to_owned(),
        tofu_version: "1.13.1".to_owned(),
        root: String::new(),
        lockfile_sha256: "a".repeat(64),
        runtime_identity_digest: format!("b3-{}", "b".repeat(64)),
        cache_path: "/tmp/runner/velnor/cache/tofu/root-123456789abc".into(),
    }
}

fn roots() -> CacheRootSet {
    CacheRootSet {
        owner_id: "normal-tofu-providers".to_owned(),
        layer: velnor_actions_contract::QualificationCacheLayer::TofuProviders,
        roots: vec![CacheRootObservation {
            id: CacheRootId::TofuPlugins,
            path: "/tmp/runner/velnor/cache/tofu/root-123456789abc".into(),
        }],
    }
}

fn manifest(runtime: &ValidatedTofuRuntime, roots: &CacheRootSet) -> PayloadManifest {
    let key = super::super::super::normal_tofu::cache_key(
        &runtime.target,
        &runtime.tofu_version,
        &runtime.root,
        &runtime.lockfile_sha256,
    )
    .expect("cache key");
    PayloadManifest::for_normal_tofu_save(
        &key,
        runtime,
        &format!("b3-{}", "c".repeat(64)),
        roots,
        vec![PayloadRoot {
            id: CacheRootId::TofuPlugins,
            present: false,
            mode: None,
            digest: None,
        }],
    )
    .expect("normal ToFu manifest")
}

#[test]
fn normal_tofu_restore_manifest_binds_scope_key_identity_and_root_set() {
    let runtime = runtime();
    let roots = roots();
    let manifest = manifest(&runtime, &roots);
    let text = manifest.canonical_json().expect("canonical manifest");
    let key = manifest.cache_key.clone();

    let parsed = PayloadManifest::parse_for_normal_tofu_restore(&text, &key, &runtime, &roots)
        .expect("bound restore manifest");
    assert_eq!(parsed, manifest);
    assert_eq!(
        PayloadManifest::parse_for_normal_tofu_restore(
            &text,
            "velnor-v1-tofu-providers-payload-v3-wrong",
            &runtime,
            &roots,
        ),
        Err("normal_tofu_manifest_binding_mismatch")
    );

    let mut changed_runtime = runtime.clone();
    changed_runtime.runtime_identity_digest = format!("b3-{}", "d".repeat(64));
    assert_eq!(
        PayloadManifest::parse_for_normal_tofu_restore(&text, &key, &changed_runtime, &roots),
        Err("normal_tofu_manifest_binding_mismatch")
    );

    let mut changed_roots = roots.clone();
    changed_roots.roots[0].id = CacheRootId::MbxActions;
    assert_eq!(
        PayloadManifest::parse_for_normal_tofu_restore(&text, &key, &runtime, &changed_roots),
        Err("normal_tofu_manifest_binding_mismatch")
    );
}
