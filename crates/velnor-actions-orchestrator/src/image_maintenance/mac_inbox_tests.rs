// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::json;
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;

fn entry(path: &str, mode: &str, kind: &str) -> GitEntry {
    GitEntry {
        path: path.to_owned(),
        mode: mode.to_owned(),
        kind: kind.to_owned(),
        sha: "a".repeat(40),
    }
}
fn tree(entries: Vec<GitEntry>) -> GitTree {
    GitTree {
        sha: "b".repeat(40),
        truncated: false,
        tree: entries,
    }
}
fn path(manifest: &str, transport: &str) -> String {
    format!("proofs/7/2/{manifest}/{transport}.json")
}

fn proof_entries(path: &str) -> Vec<GitEntry> {
    let mut result = Vec::new();
    for (index, _) in path.match_indices('/') {
        result.push(entry(&path[..index], "040000", "tree"));
    }
    result.push(entry(path, "100644", "blob"));
    result
}

#[test]
fn data_only_tree_selects_only_the_exact_release_proof() -> TestResult {
    let manifest = "c".repeat(64);
    let transport = "d".repeat(64);
    let target = path(&manifest, &transport);
    let select = |tree| select_entry(tree, &"b".repeat(40), 7, 2, &manifest);
    let chosen = select(tree(proof_entries(&target)))?.ok_or("proof missing")?;
    assert_eq!(chosen.path, target);
    for other in ["proofs/8/2", "proofs/7/3", "proofs/7/2"] {
        let alternate = format!("{other}/{}/{}.json", "e".repeat(64), transport);
        assert!(select(tree(proof_entries(&alternate)))?.is_none());
    }
    let mut truncated = tree(vec![entry(&target, "100644", "blob")]);
    truncated.truncated = true;
    assert!(select(truncated).is_err());
    let wrong_sha = GitTree {
        sha: "f".repeat(40),
        truncated: false,
        tree: vec![],
    };
    assert!(select(wrong_sha).is_err());
    assert!(
        select(tree(
            (0..20_001)
                .map(|_| entry("proofs", "040000", "tree"))
                .collect()
        ))
        .is_err()
    );
    for invalid_entry in [
        entry(".github/workflows/publish.yml", "100644", "blob"),
        entry("README.md", "100644", "blob"),
        entry(&target, "100755", "blob"),
        entry(&target, "120000", "blob"),
        entry(&target, "160000", "commit"),
        entry("proofs/07", "040000", "tree"),
        entry("proofs/0", "040000", "tree"),
        entry("proofs/7/00", "040000", "tree"),
        entry("proofs/7/2/not-a-digest", "040000", "tree"),
    ] {
        assert!(
            select(tree(vec![invalid_entry])).is_err(),
            "invalid tree entry accepted"
        );
    }
    assert!(
        select(tree(vec![
            entry(&target, "100644", "blob"),
            entry(&target, "100644", "blob")
        ]))
        .is_err()
    );
    let mut ambiguous = proof_entries(&target);
    ambiguous.push(entry(&path(&manifest, &"e".repeat(64)), "100644", "blob"));
    assert!(select(tree(ambiguous)).is_err());
    Ok(())
}

fn test_blob(bytes: &[u8]) -> Result<(GitBlob, GitEntry, String), OrchestratorError> {
    let sha = git_blob_sha(bytes)?;
    let digest = hex_digest(&Sha256::digest(bytes));
    let encoded = STANDARD.encode(bytes);
    let content = format!(
        "{}\r\n{}\n",
        &encoded[..encoded.len() / 2],
        &encoded[encoded.len() / 2..]
    );
    Ok((
        GitBlob {
            sha: sha.clone(),
            size: bytes.len(),
            encoding: "base64".to_owned(),
            content,
        },
        GitEntry {
            path: format!("{digest}.json"),
            mode: "100644".to_owned(),
            kind: "blob".to_owned(),
            sha,
        },
        digest,
    ))
}
fn decode_bytes(bytes: &[u8]) -> Result<Proof, OrchestratorError> {
    let (blob, entry, digest) = test_blob(bytes)?;
    decode_proof(&blob, &entry, &digest)
}

#[test]
fn transport_requires_exact_git_and_content_hashes_and_closed_bounded_payloads() -> TestResult {
    let transport = json!({"schema":"velnor.runner-image-mac-proof-transport.v1", "envelope_base64":STANDARD.encode(b"envelope"), "sigstore_bundle_base64":STANDARD.encode(b"bundle")});
    let valid = serde_json::to_vec(&transport)?;
    let decoded = decode_bytes(&valid)?;
    assert_eq!(decoded.envelope, b"envelope");
    assert_eq!(decoded.bundle, b"bundle");
    let (mut blob, entry, digest) = test_blob(&valid)?;
    blob.size += 1;
    assert!(
        decode_proof(&blob, &entry, &digest).is_err(),
        "wrong Git blob size accepted"
    );
    let (mut blob, entry, digest) = test_blob(&valid)?;
    blob.sha = "f".repeat(40);
    assert!(
        decode_proof(&blob, &entry, &digest).is_err(),
        "wrong Git blob identity accepted"
    );
    let (blob, mut entry, digest) = test_blob(&valid)?;
    entry.sha = "f".repeat(40);
    assert!(
        decode_proof(&blob, &entry, &digest).is_err(),
        "wrong tree blob hash accepted"
    );
    let (mut blob, entry, _) = test_blob(&valid)?;
    let mut altered = valid.clone();
    altered[0] ^= 1;
    blob.content = STANDARD.encode(&altered);
    assert!(
        decode_proof(&blob, &entry, &hex_digest(&Sha256::digest(&altered))).is_err(),
        "forged Git blob bytes accepted"
    );
    let (blob, entry, _) = test_blob(&valid)?;
    assert!(
        decode_proof(&blob, &entry, &"f".repeat(64)).is_err(),
        "wrong content address accepted"
    );
    let raw = String::from_utf8(valid)?;
    let duplicate = raw.replacen('{', "{\"schema\":\"duplicate\",", 1);
    assert!(
        decode_bytes(duplicate.as_bytes()).is_err(),
        "duplicate schema accepted"
    );
    for (field, value) in [
        ("schema", json!("wrong")),
        ("envelope_base64", json!("%%%")),
        ("sigstore_bundle_base64", json!("")),
        ("extra", json!(true)),
    ] {
        let mut changed = transport.clone();
        changed[field] = value;
        assert!(
            decode_bytes(&serde_json::to_vec(&changed)?).is_err(),
            "invalid {field} accepted"
        );
    }
    let mut oversized_envelope = transport.clone();
    oversized_envelope["envelope_base64"] = json!(STANDARD.encode(vec![0; ENVELOPE_LIMIT + 1]));
    assert!(
        decode_bytes(&serde_json::to_vec(&oversized_envelope)?).is_err(),
        "oversized envelope accepted"
    );
    let mut huge = transport;
    huge["sigstore_bundle_base64"] = json!(STANDARD.encode(vec![0; BUNDLE_LIMIT + 1]));
    assert!(
        decode_bytes(&serde_json::to_vec(&huge)?).is_err(),
        "oversized bundle accepted"
    );
    Ok(())
}
