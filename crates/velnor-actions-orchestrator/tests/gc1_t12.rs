// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    PRIVATE_FETCH_OPERATION, PRIVATE_FETCH_TOKEN_ENV, PRIVATE_TOKEN_OUTPUT_EXPR, Step, StepId,
    StepKind, StepRole,
};
use velnor_actions_mise::cache_sources::{
    ByteCounter, ImmutableSourceObject, SourceByteLedger, SourceHydration, decide_hydration,
    is_approved_origin, is_never_archive_path, require_approved_origins, verify_restored_object,
};
use velnor_actions_orchestrator::{
    CheckoutState, EntryKind, ProjectionEntry, RealMtime, RestoreDecision, authenticate_entry,
    decide, install_staged_file, reconcile, sha256_hex, staged_path,
};
use velnor_actions_workflow_renderer::private_source_steps::{
    PRIVATE_FETCH_NAME, PRIVATE_MINT_NAME, PRIVATE_OWNER_ENV, PRIVATE_REPOSITORIES_ENV,
    PRIVATE_REVOKE_CONDITION, PRIVATE_REVOKE_NAME, PRIVATE_ROOTS_ENV, PRIVATE_SOURCES_ENV,
    PrivateSourcesSpec, private_source_steps,
};

mod toolchain_expressions;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

struct Parts {
    owner: String,
    app_id: String,
    app_key: String,
    repositories: Vec<String>,
    roots: Vec<String>,
    toolchain: BTreeMap<String, String>,
}

fn valid_parts() -> Parts {
    Parts {
        owner: "octo-org".to_owned(),
        app_id: "VELNOR_APP_ID".to_owned(),
        app_key: "VELNOR_APP_PRIVATE_KEY".to_owned(),
        repositories: vec!["deps-a".to_owned(), "deps-b".to_owned()],
        roots: vec![".".to_owned(), "vendor".to_owned()],
        toolchain: BTreeMap::from([
            ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
            ("MISE_RUSTUP_HOME".to_owned(), "/owned/rustup".to_owned()),
        ]),
    }
}

fn render(parts: &Parts, kinds: &[&str]) -> Result<[Step; 3], Box<dyn Error>> {
    let spec = PrivateSourcesSpec {
        owner: parts.owner.as_str(),
        app_id_variable: parts.app_id.as_str(),
        app_private_key_secret: parts.app_key.as_str(),
        repositories: parts.repositories.as_slice(),
        kinds,
        roots: parts.roots.as_slice(),
        toolchain_env: &parts.toolchain,
    };
    Ok(private_source_steps(&spec).map_err(io::Error::other)?)
}

fn step_envs(step: &Step) -> Vec<(&String, &String)> {
    match &step.kind {
        StepKind::Action { env, .. }
        | StepKind::Shell { env, .. }
        | StepKind::TaskExecution { env, .. }
        | StepKind::Internal { env, .. } => env.iter().collect(),
        StepKind::NativeCheckExecution { .. } => Vec::new(),
    }
}

fn check_triple_ordering() -> Result<(), Box<dyn Error>> {
    let parts = valid_parts();
    let steps = render(&parts, &["cargo", "release-assets"])?;
    if steps[0].name != PRIVATE_MINT_NAME
        || steps[1].name != PRIVATE_FETCH_NAME
        || steps[2].name != PRIVATE_REVOKE_NAME
    {
        return Err(fail("private block names drifted"));
    }
    let roles: Vec<Option<StepRole>> = steps.iter().map(|step| step.role).collect();
    if roles
        != [
            Some(StepRole::PrivateTokenMint),
            Some(StepRole::PrivateFetch),
            Some(StepRole::PrivateRevoke),
        ]
    {
        return Err(fail("mint fetch revoke ordering drifted"));
    }
    if steps[0].id != Some(StepId::DepsToken) || steps[1].id.is_some() || steps[2].id.is_some() {
        return Err(fail("private block step ids drifted"));
    }
    if steps[2].condition.as_deref() != Some(PRIVATE_REVOKE_CONDITION) {
        return Err(fail("revoke after mint drifted"));
    }
    Ok(())
}

fn check_mint_carries_no_credential() -> Result<(), Box<dyn Error>> {
    let parts = valid_parts();
    let steps = render(&parts, &["cargo", "release-assets"])?;
    let StepKind::Action { with, env, .. } = &steps[0].kind else {
        return Err(fail("mint step kind drifted"));
    };
    if !env.is_empty() {
        return Err(fail("mint step leaks env into payload"));
    }
    if with.get("owner").map(String::as_str) != Some("octo-org")
        || with.get("repositories").map(String::as_str) != Some("deps-a,deps-b")
    {
        return Err(fail("mint step lost its audience binding"));
    }
    if with
        .values()
        .any(|value| value.contains(PRIVATE_TOKEN_OUTPUT_EXPR))
    {
        return Err(fail("mint step exposes the token output"));
    }
    Ok(())
}

fn check_fetch_and_revoke_token_binding() -> Result<(), Box<dyn Error>> {
    let parts = valid_parts();
    let steps = render(&parts, &["cargo", "release-assets"])?;
    let StepKind::Internal { operation, env } = &steps[1].kind else {
        return Err(fail("fetch step kind drifted"));
    };
    if operation != PRIVATE_FETCH_OPERATION {
        return Err(fail("fetch operation drifted"));
    }
    if env.get(PRIVATE_FETCH_TOKEN_ENV).map(String::as_str) != Some(PRIVATE_TOKEN_OUTPUT_EXPR)
        || env.get(PRIVATE_OWNER_ENV).map(String::as_str) != Some("octo-org")
        || env.get(PRIVATE_REPOSITORIES_ENV).map(String::as_str) != Some("deps-a,deps-b")
        || env.get(PRIVATE_SOURCES_ENV).map(String::as_str) != Some("cargo,release-assets")
        || env.get(PRIVATE_ROOTS_ENV).map(String::as_str) != Some(".,vendor")
        || env.get("MISE_NO_CONFIG").map(String::as_str) != Some("1")
    {
        return Err(fail("fetch payload drifted"));
    }
    let StepKind::Internal { operation, env } = &steps[2].kind else {
        return Err(fail("revoke step kind drifted"));
    };
    if operation != velnor_actions_contract::PRIVATE_REVOKE_OPERATION {
        return Err(fail("revoke operation drifted"));
    }
    if env.len() != 1
        || env.get(PRIVATE_FETCH_TOKEN_ENV).map(String::as_str) != Some(PRIVATE_TOKEN_OUTPUT_EXPR)
    {
        return Err(fail("revoke payload drifted"));
    }
    for step in &steps {
        for (key, value) in step_envs(step) {
            if value.contains(PRIVATE_TOKEN_OUTPUT_EXPR) && key != PRIVATE_FETCH_TOKEN_ENV {
                return Err(fail("token output escapes its binding"));
            }
        }
        if let StepKind::Action { with, .. } = &step.kind
            && with
                .values()
                .any(|value| value.contains(PRIVATE_TOKEN_OUTPUT_EXPR))
        {
            return Err(fail("token output escapes into action inputs"));
        }
    }
    Ok(())
}

fn check_allowlist_gates_payload() -> Result<(), Box<dyn Error>> {
    let mut parts = valid_parts();
    parts.repositories.clear();
    if render(&parts, &["cargo"]).is_ok() {
        return Err(fail("private payload admitted without readers"));
    }
    let mut parts = valid_parts();
    parts.repositories = vec!["deps-b".to_owned(), "deps-a".to_owned()];
    if render(&parts, &["cargo"]).is_ok() {
        return Err(fail("unordered allowlist admitted"));
    }
    Ok(())
}

fn private_asset_entry(path: &str, bytes: &[u8]) -> ProjectionEntry {
    ProjectionEntry {
        path: path.to_owned(),
        kind: EntryKind::File,
        mode: 0o644,
        content_sha256: sha256_hex(bytes),
        link_target: None,
        mtime: Some(RealMtime {
            seconds: 946_684_800,
            nanoseconds: 123_456_789,
        }),
    }
}

fn private_asset_object(id: &str, repository: &str, bytes: &[u8]) -> ImmutableSourceObject {
    ImmutableSourceObject {
        object_id: id.to_owned(),
        kind: "private_asset".to_owned(),
        approved_origin_id: repository.to_owned(),
        content_digest: format!("s256-{}", sha256_hex(bytes)),
    }
}

fn write_file(root: &Path, path: &str, bytes: &[u8]) -> Result<PathBuf, Box<dyn Error>> {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&full, bytes)?;
    fs::set_permissions(&full, fs::Permissions::from_mode(0o644))?;
    Ok(full)
}

fn check_approved_assets_restore_at_stable_paths() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let bytes = b"private asset bytes\n";
    let declared = private_asset_entry("deps/vendor/blob.bin", bytes);
    write_file(&checkout, "deps/vendor/blob.bin", bytes)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Unchanged {
        return Err(fail("matching private checkout rejected"));
    }
    let object = private_asset_object("vendor-blob", "octo-org/deps-a", bytes);
    require_approved_origins(std::slice::from_ref(&object), &["octo-org/deps-a"])?;
    if !is_approved_origin("octo-org/deps-a", &["octo-org/deps-a"]) {
        return Err(fail("approved origin refused"));
    }
    if !verify_restored_object(&object, true, bytes)? {
        return Err(fail("restored private bytes rejected"));
    }
    let staged_file = write_file(&staged, "deps/vendor/blob.bin", bytes)?;
    reconcile(&declared, &declared)?;
    if decide(Some(&declared), Some(&declared), CheckoutState::Unchanged)
        != RestoreDecision::Restore
    {
        return Err(fail("approved asset not scheduled"));
    }
    let destination = staged_path(&checkout, "deps/vendor/blob.bin")?;
    if !destination.starts_with(&checkout) {
        return Err(fail("stable path escapes the checkout"));
    }
    let outcome = install_staged_file(&staged_file, &destination, &declared)?;
    if !outcome.mtime_restored || fs::read(&destination)? != bytes {
        return Err(fail("restored private asset drifted"));
    }
    let present = vec!["vendor-blob".to_owned()];
    if decide_hydration(&[object], &present)? != SourceHydration::Warm {
        return Err(fail("covered private asset not warm"));
    }
    let mut ledger = SourceByteLedger::new(
        ByteCounter::unknown("origin_acquisition_not_needed"),
        ByteCounter::measured(0),
    );
    ledger.validate()?;
    ledger.record_cache(u64::try_from(bytes.len()).map_err(|_| fail("asset too large"))?)?;
    if ledger.record_origin(1).is_ok() {
        return Err(fail("unknown origin bytes admitted a write"));
    }
    if ledger.origin_bytes.value.is_some() || ledger.cache_bytes.value != Some(20) {
        return Err(fail("origin and cache bytes conflated"));
    }
    Ok(())
}

#[test]
fn gc1_t12_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_triple_ordering()?;
    check_mint_carries_no_credential()?;
    check_fetch_and_revoke_token_binding()?;
    toolchain_expressions::check_generated_toolchain_expressions()?;
    check_allowlist_gates_payload()?;
    check_approved_assets_restore_at_stable_paths()?;
    Ok(())
}

fn reject_parts(label: &str, parts: &Parts, kinds: &[&str]) -> Result<(), Box<dyn Error>> {
    if render(parts, kinds).is_ok() {
        return Err(fail(&format!("invalid private spec admitted: {label}")));
    }
    Ok(())
}

fn check_rejects_unauthorized_payloads() -> Result<(), Box<dyn Error>> {
    let mut credential_key = valid_parts();
    credential_key
        .toolchain
        .insert("GITHUB_TOKEN".to_owned(), "cached-secret".to_owned());
    reject_parts("cached credential key", &credential_key, &["cargo"])?;
    let mut endpoint_key = valid_parts();
    endpoint_key
        .toolchain
        .insert("GH_HOST".to_owned(), "example.com".to_owned());
    reject_parts("endpoint override key", &endpoint_key, &["cargo"])?;
    let mut namespaced_key = valid_parts();
    namespaced_key
        .toolchain
        .insert("VELNOR_EXTRA".to_owned(), "1".to_owned());
    reject_parts("namespaced key", &namespaced_key, &["cargo"])?;
    let mut smuggled_token = valid_parts();
    smuggled_token.toolchain.insert(
        "MISE_NO_CONFIG".to_owned(),
        "x ${{ steps.deps-token.outputs.token }}".to_owned(),
    );
    reject_parts("smuggled token expression", &smuggled_token, &["cargo"])?;
    let mut bad_owner = valid_parts();
    "not an owner!".clone_into(&mut bad_owner.owner);
    reject_parts("bad owner", &bad_owner, &["cargo"])?;
    let mut bad_handle = valid_parts();
    "lowercase".clone_into(&mut bad_handle.app_id);
    reject_parts("bad handle", &bad_handle, &["cargo"])?;
    reject_parts("unknown kind", &valid_parts(), &["tarball"])?;
    let mut bad_root = valid_parts();
    bad_root.roots = vec!["../escape".to_owned()];
    reject_parts("escaping root", &bad_root, &["cargo"])?;
    Ok(())
}

fn check_rejects_unapproved_or_tampered_assets() -> Result<(), Box<dyn Error>> {
    let bytes = b"private asset bytes\n";
    let object = private_asset_object("vendor-blob", "octo-org/deps-a", bytes);
    if require_approved_origins(std::slice::from_ref(&object), &["evil-org/other"]).is_ok() {
        return Err(fail("unapproved origin admitted"));
    }
    if verify_restored_object(&object, true, b"tampered bytes\n")? {
        return Err(fail("tampered private bytes verified"));
    }
    if verify_restored_object(&object, false, bytes)? {
        return Err(fail("unrestored private bytes trusted"));
    }
    let duplicated = vec![object.clone(), object.clone()];
    if decide_hydration(&duplicated, &[]).is_ok() {
        return Err(fail("duplicate private object admitted"));
    }
    let mut bad_digest = object.clone();
    bad_digest.content_digest = sha256_hex(bytes);
    if require_approved_origins(&[bad_digest], &["octo-org/deps-a"]).is_ok() {
        return Err(fail("digest without prefix admitted"));
    }
    if !is_never_archive_path("deps/credentials/token") {
        return Err(fail("credential path not refused"));
    }
    if velnor_actions_orchestrator::validate_relative_path("deps/credentials/token").is_err() {
        return Err(fail("stable relative path rejected"));
    }
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let declared = private_asset_entry("deps/vendor/blob.bin", bytes);
    write_file(&checkout, "deps/vendor/blob.bin", bytes)?;
    let staged_file = write_file(&staged, "deps/vendor/blob.bin", b"tampered bytes\n")?;
    let destination = staged_path(&checkout, "deps/vendor/blob.bin")?;
    if install_staged_file(&staged_file, &destination, &declared).is_ok() {
        return Err(fail("tampered staged asset installed"));
    }
    if fs::read(&destination)? != bytes {
        return Err(fail("failed install wrote destination"));
    }
    let mut unknown = ByteCounter::unknown("origin_acquisition_not_needed");
    if unknown.record(1).is_ok() || unknown.value.is_some() {
        return Err(fail("unknown origin bytes zero-filled"));
    }
    Ok(())
}

#[test]
fn gc1_t12_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_unauthorized_payloads()?;
    toolchain_expressions::check_rejects_unowned_toolchain_expressions()?;
    check_rejects_unapproved_or_tampered_assets()?;
    Ok(())
}
