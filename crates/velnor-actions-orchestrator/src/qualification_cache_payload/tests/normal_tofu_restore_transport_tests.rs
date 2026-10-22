use super::super::super::super::manifest::PayloadManifest;
use super::super::super::super::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use super::super::super::NormalTofuProvidersCachePayloadRequest;
use super::super::super::ValidatedTofuRuntime;
use super::{StageContext, StageReceiptClaim, validate_cache_hit_payload};
use crate::qualification_cache_payload::transfer::{
    snapshot_roots, useful_state_digest, write_transport_payload,
};
use std::io::{Cursor, Write};
use std::path::PathBuf;
use tempfile::TempDir;

struct Fixture {
    _temporary: TempDir,
    runner_temp: PathBuf,
    workspace: PathBuf,
    cache_path: PathBuf,
    cache_key: String,
    runtime: ValidatedTofuRuntime,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = std::fs::canonicalize(temporary.path()).expect("canonical temporary root");
        let runner_temp = root.join("runner-temp");
        let workspace = root.join("workspace");
        let cache_path = runner_temp.join("velnor/cache/tofu/root-123456789abc");
        std::fs::create_dir_all(&runner_temp).expect("runner temp");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::create_dir_all(&cache_path).expect("provider cache");
        std::fs::write(cache_path.join("provider.zip"), b"checked provider bytes")
            .expect("provider archive");
        let cache_key = super::super::super::cache_key(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "",
            &"a".repeat(64),
        )
        .expect("cache key");
        let runtime = ValidatedTofuRuntime {
            target: "x86_64-unknown-linux-gnu".to_owned(),
            tofu_version: "1.13.1".to_owned(),
            root: String::new(),
            lockfile_sha256: "a".repeat(64),
            runtime_identity_digest: format!("b3-{}", "b".repeat(64)),
            cache_path: cache_path.clone(),
        };
        Self {
            _temporary: temporary,
            runner_temp,
            workspace,
            cache_path,
            cache_key,
            runtime,
        }
    }

    fn request(&self) -> NormalTofuProvidersCachePayloadRequest<'_> {
        NormalTofuProvidersCachePayloadRequest {
            runner_temp: &self.runner_temp,
            workspace: &self.workspace,
            expected_target: &self.runtime.target,
            expected_tofu_version: &self.runtime.tofu_version,
            expected_root: &self.runtime.root,
            cache_key: &self.cache_key,
            cache_path: &self.cache_path,
            configured_plugin_cache_dir: &self.cache_path,
            runtime_observation: "{}",
            previous_stage_claim: None,
            operation_limit_bytes: 4 * 1024 * 1024,
        }
    }

    fn roots(&self) -> CacheRootSet {
        CacheRootSet {
            owner_id: "normal-tofu-providers".to_owned(),
            layer: velnor_actions_contract::QualificationCacheLayer::TofuProviders,
            roots: vec![CacheRootObservation {
                id: CacheRootId::TofuPlugins,
                path: self.cache_path.clone(),
            }],
        }
    }

    fn manifest(&self, runtime: &ValidatedTofuRuntime, roots: &CacheRootSet) -> PayloadManifest {
        let payload_roots = snapshot_roots(roots).expect("root inventory");
        let state_digest = useful_state_digest(&payload_roots).expect("state digest");
        PayloadManifest::for_normal_tofu_save(
            &self.cache_key,
            runtime,
            &state_digest,
            roots,
            payload_roots,
        )
        .expect("normal ToFu manifest")
    }

    fn stage_payload(
        &self,
        manifest: &PayloadManifest,
        roots: &CacheRootSet,
        corrupt: bool,
    ) -> StageReceiptClaim {
        let mut bytes = self.transport_payload(manifest, roots);
        if corrupt {
            let index = bytes
                .windows(b"checked provider bytes".len())
                .position(|window| window == b"checked provider bytes")
                .expect("provider member bytes");
            bytes[index] ^= 1;
        }
        self.stage_bytes(&bytes)
    }

    fn transport_payload(&self, manifest: &PayloadManifest, roots: &CacheRootSet) -> Vec<u8> {
        let manifest_text = manifest.canonical_json().expect("canonical manifest");
        let mut transport = Cursor::new(Vec::new());
        write_transport_payload(
            &mut transport,
            self.request().operation_limit_bytes,
            roots,
            &manifest.roots,
            &manifest_text,
        )
        .expect("write bounded transport");
        transport.into_inner()
    }

    fn stage_bytes(&self, bytes: &[u8]) -> StageReceiptClaim {
        let context = StageContext::normal_tofu_providers(
            &self.runner_temp,
            &self.workspace,
            &self.cache_key,
        )
        .expect("stage context");
        let mut writer = crate::cache_action_stage::create_stage(&context).expect("stage writer");
        writer.write_all(bytes).expect("write cache transport");
        writer.finish().expect("claim cache transport")
    }
}

fn replace_transport_bytes(bytes: &mut [u8], from: &[u8], to: &[u8]) {
    assert_eq!(
        from.len(),
        to.len(),
        "replacement must preserve tar offsets"
    );
    let matches = bytes
        .windows(from.len())
        .enumerate()
        .filter_map(|(index, candidate)| (candidate == from).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "replacement must be unambiguous");
    bytes[matches[0]..matches[0] + to.len()].copy_from_slice(to);
}

fn change_tar_entry_type(bytes: &mut [u8], path: &str, entry_type: u8) {
    const ENVELOPE_BYTES: usize = 34;
    const TAR_BLOCK_BYTES: usize = 512;
    let header_offset = {
        let mut archive = tar::Archive::new(Cursor::new(&bytes[ENVELOPE_BYTES..]));
        archive
            .entries()
            .expect("tar entries")
            .find_map(|item| {
                let entry = item.expect("tar entry");
                (entry.path().ok()?.to_str()? == path).then(|| entry.raw_header_position())
            })
            .expect("tar entry path")
    };
    let header_start = ENVELOPE_BYTES + usize::try_from(header_offset).expect("header offset");
    let header_end = header_start + TAR_BLOCK_BYTES;
    let header = &mut bytes[header_start..header_end];
    header[156] = entry_type;
    header[148..156].fill(b' ');
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let encoded_checksum = format!("{checksum:06o}\0 ");
    assert_eq!(encoded_checksum.len(), 8);
    header[148..156].copy_from_slice(encoded_checksum.as_bytes());
}

#[test]
fn normal_tofu_cache_hit_preflights_bound_stage_and_returns_state_digest() {
    let fixture = Fixture::new();
    let roots = fixture.roots();
    let manifest = fixture.manifest(&fixture.runtime, &roots);
    let claim = fixture.stage_payload(&manifest, &roots, false);

    assert_eq!(
        validate_cache_hit_payload(&fixture.request(), &fixture.runtime, &claim),
        Ok(manifest.state_digest)
    );
}

#[test]
fn normal_tofu_cache_hit_rejects_swapped_runtime_and_member_digest() {
    let fixture = Fixture::new();
    let roots = fixture.roots();
    let manifest = fixture.manifest(&fixture.runtime, &roots);
    let claim = fixture.stage_payload(&manifest, &roots, false);
    let mut swapped_runtime = fixture.runtime.clone();
    swapped_runtime.runtime_identity_digest = format!("b3-{}", "c".repeat(64));
    assert_eq!(
        validate_cache_hit_payload(&fixture.request(), &swapped_runtime, &claim),
        Err("normal_tofu_manifest_binding_mismatch")
    );

    let corrupted_fixture = Fixture::new();
    let corrupted_roots = corrupted_fixture.roots();
    let corrupted_manifest =
        corrupted_fixture.manifest(&corrupted_fixture.runtime, &corrupted_roots);
    let corrupted_claim =
        corrupted_fixture.stage_payload(&corrupted_manifest, &corrupted_roots, true);
    assert_eq!(
        validate_cache_hit_payload(
            &corrupted_fixture.request(),
            &corrupted_fixture.runtime,
            &corrupted_claim
        ),
        Err("qualification_cache_payload_digest_mismatch")
    );
}

#[test]
fn normal_tofu_cache_hit_rejects_manifest_key_mismatch() {
    let fixture = Fixture::new();
    let roots = fixture.roots();
    let mut manifest = fixture.manifest(&fixture.runtime, &roots);
    manifest.cache_key = "velnor-v1-tofu-providers-payload-v3-wrong".to_owned();
    let claim = fixture.stage_payload(&manifest, &roots, false);
    assert_eq!(
        validate_cache_hit_payload(&fixture.request(), &fixture.runtime, &claim),
        Err("normal_tofu_manifest_binding_mismatch")
    );
}

#[test]
fn normal_tofu_cache_hit_accepts_explicit_nested_directories() {
    let fixture = Fixture::new();
    let nested = fixture.cache_path.join("a/b");
    std::fs::create_dir_all(&nested).expect("nested directories");
    std::fs::write(nested.join("child"), b"nested provider bytes").expect("nested file");
    let roots = fixture.roots();
    let manifest = fixture.manifest(&fixture.runtime, &roots);
    let claim = fixture.stage_payload(&manifest, &roots, false);

    assert_eq!(
        validate_cache_hit_payload(&fixture.request(), &fixture.runtime, &claim),
        Ok(manifest.state_digest)
    );
}

#[test]
fn normal_tofu_cache_hit_rejects_file_used_as_parent_directory() {
    let fixture = Fixture::new();
    let nested = fixture.cache_path.join("a/b");
    std::fs::create_dir_all(&nested).expect("nested directories");
    std::fs::write(nested.join("child"), b"nested provider bytes").expect("nested file");
    let roots = fixture.roots();
    let manifest = fixture.manifest(&fixture.runtime, &roots);
    let mut transport = fixture.transport_payload(&manifest, &roots);
    change_tar_entry_type(&mut transport, "roots/tofu_plugins/a", b'0');
    let claim = fixture.stage_bytes(&transport);

    assert_eq!(
        validate_cache_hit_payload(&fixture.request(), &fixture.runtime, &claim),
        Err("qualification_cache_payload_parent_missing")
    );
}

#[cfg(unix)]
#[test]
fn normal_tofu_cache_hit_rejects_symlink_used_as_parent_directory() {
    let fixture = Fixture::new();
    std::fs::write(fixture.cache_path.join("bbbbbbb"), b"nested provider bytes")
        .expect("target file");
    std::os::unix::fs::symlink("bbbbbbb", fixture.cache_path.join("a")).expect("parent symlink");
    let roots = fixture.roots();
    let manifest = fixture.manifest(&fixture.runtime, &roots);
    let mut transport = fixture.transport_payload(&manifest, &roots);
    replace_transport_bytes(
        &mut transport,
        b"roots/tofu_plugins/bbbbbbb",
        b"roots/tofu_plugins/a/child",
    );
    let claim = fixture.stage_bytes(&transport);

    assert_eq!(
        validate_cache_hit_payload(&fixture.request(), &fixture.runtime, &claim),
        Err("qualification_cache_payload_parent_missing")
    );
}

#[test]
fn normal_tofu_cache_hit_rejects_traversal_archive_path() {
    let fixture = Fixture::new();
    let nested = fixture.cache_path.join("z/a");
    std::fs::create_dir_all(&nested).expect("nested directories");
    std::fs::write(nested.join("bbbb"), b"nested provider bytes").expect("nested file");
    let roots = fixture.roots();
    let manifest = fixture.manifest(&fixture.runtime, &roots);
    let mut transport = fixture.transport_payload(&manifest, &roots);
    replace_transport_bytes(
        &mut transport,
        b"roots/tofu_plugins/z/a/bbbb",
        b"roots/tofu_plugins/z/a/../x",
    );
    let claim = fixture.stage_bytes(&transport);

    assert_eq!(
        validate_cache_hit_payload(&fixture.request(), &fixture.runtime, &claim),
        Err("qualification_cache_payload_path_invalid")
    );
}

#[cfg(unix)]
#[test]
fn normal_tofu_cache_hit_rejects_escaping_symlink_target() {
    let fixture = Fixture::new();
    std::fs::write(fixture.cache_path.join("target"), b"valid target").expect("target file");
    std::os::unix::fs::symlink("target", fixture.cache_path.join("escape")).expect("symlink");
    let roots = fixture.roots();
    let manifest = fixture.manifest(&fixture.runtime, &roots);
    let mut transport = fixture.transport_payload(&manifest, &roots);
    replace_transport_bytes(&mut transport, b"linkpath=target", b"linkpath=../x/y");
    let claim = fixture.stage_bytes(&transport);

    assert_eq!(
        validate_cache_hit_payload(&fixture.request(), &fixture.runtime, &claim),
        Err("qualification_cache_payload_symlink_escape")
    );
}

#[test]
fn normal_tofu_cache_hit_rejects_duplicate_archive_path_order() {
    let fixture = Fixture::new();
    std::fs::write(fixture.cache_path.join("a"), b"first provider").expect("first file");
    std::fs::write(fixture.cache_path.join("b"), b"second provider").expect("second file");
    let roots = fixture.roots();
    let manifest = fixture.manifest(&fixture.runtime, &roots);
    let mut transport = fixture.transport_payload(&manifest, &roots);
    replace_transport_bytes(
        &mut transport,
        b"roots/tofu_plugins/b",
        b"roots/tofu_plugins/a",
    );
    let claim = fixture.stage_bytes(&transport);

    assert_eq!(
        validate_cache_hit_payload(&fixture.request(), &fixture.runtime, &claim),
        Err("qualification_cache_payload_path_order_invalid")
    );
}
