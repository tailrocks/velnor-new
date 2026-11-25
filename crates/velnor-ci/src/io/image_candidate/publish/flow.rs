// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::super::process::Runner;
use super::super::{Checked, FileSource, check_record, output};
use super::gh::Gh;
use super::registry::{MAX_BODY, Registry, fail};
use super::{Run, parallel};
use crate::cli::ImagePublishArgs;
use crate::error::CiError;
use crate::image_candidate::archive::Scanned;
use crate::image_candidate::hashing::sha256_hex;
use crate::image_candidate::json::parse;
use crate::image_candidate::oci::BlobSource;
use crate::image_publish::checks::{
    artifact, artifacts_path, attestation, attestation_args, main_tip, repository, repository_path,
    tip_path,
};
use crate::image_publish::digest::Digest;
use crate::image_publish::evidence::{Published, encode};
use crate::image_publish::gate::{Identity, Policy, identity, record_matches};
use crate::image_publish::location::Kind;
use crate::image_publish::plan::{Blob, blobs};

const PASS: &str = "PASS: Publication completed for the job image with its OCI digest. Package access is `NOT_RUN`.\n";

struct Paths {
    archive: PathBuf,
    record: PathBuf,
    result: PathBuf,
}

fn regular_input(path: &Path) -> Result<PathBuf, CiError> {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return fail("Candidate inputs must be regular files.");
    }
    fs::canonicalize(path).map_err(|error| CiError::new(format!("{}: {error}", path.display())))
}

fn paths(args: &ImagePublishArgs) -> Result<Paths, CiError> {
    let requested = std::path::absolute(&args.result)
        .map_err(|error| CiError::new(format!("{}: {error}", args.result.display())))?;
    let result = output::standalone(&requested)?;
    if fs::symlink_metadata(&result).is_ok() {
        return fail("Result path must be new.");
    }
    Ok(Paths {
        archive: regular_input(&args.archive)?,
        record: regular_input(&args.record)?,
        result,
    })
}

fn current_main(gh: &Gh<'_>, identity: &Identity) -> Result<(), CiError> {
    main_tip(&gh.api(&tip_path())?, &identity.source)
}

fn read_manifest(
    source: &FileSource,
    scan: &Scanned,
    digest: &Digest,
    size: u64,
) -> Result<Vec<u8>, CiError> {
    let unreadable = || CiError::new("Cannot read OCI manifest bytes.");
    let blob = scan.blobs.get(digest.hex()).ok_or_else(unreadable)?;
    if size > MAX_BODY || blob.size != size {
        return fail("OCI manifest exceeds its size limit.");
    }
    let mut data = Vec::new();
    source
        .open(blob)
        .and_then(|mut stream| stream.read_to_end(&mut data))
        .map_err(|_| unreadable())?;
    if data.len() as u64 != size || sha256_hex(&data) != digest.hex() {
        return fail("OCI platform manifest bytes changed after validation.");
    }
    Ok(data)
}

fn upload(
    registry: &Registry<'_>,
    source: &FileSource,
    scan: &Scanned,
    planned: &[Blob],
) -> Result<(), CiError> {
    parallel::each(planned, &|blob| {
        let entry = &scan.blobs[blob.digest.hex()];
        registry.push_blob(&blob.digest, blob.size, &|| source.open(entry))
    })
}

fn recheck(registry: &Registry<'_>, planned: &[Blob]) -> Result<(), CiError> {
    parallel::each(planned, &|blob| {
        if !registry.exists(Kind::Blobs, &blob.digest, blob.size)? {
            return fail("Registry blob is absent after manifest upload.");
        }
        registry.verify(Kind::Blobs, &blob.digest, blob.size)
    })
}

fn identity_of(args: &ImagePublishArgs, run: &Run<'_>) -> Result<(Identity, String), CiError> {
    let policy = Policy::new(&args.workflow, &args.event)?;
    let current = identity(&run.vars, &policy)?;
    let actor = run.vars.get("GITHUB_ACTOR").filter(|name| !name.is_empty());
    match actor {
        Some(name) if !run.token.is_empty() => Ok((current, name.clone())),
        _ => fail("Required GitHub workflow identity is missing."),
    }
}

pub(super) fn publish(args: &ImagePublishArgs, run: &Run<'_>) -> Result<String, CiError> {
    let (identity, actor) = identity_of(args, run)?;
    let producer = Policy::producer()?;
    let runner = Runner::start()?;
    let paths = paths(args)?;
    let gh = Gh {
        runner: &runner,
        program: run.gh,
    };
    repository(&gh.api(&repository_path())?)?;
    current_main(&gh, &identity)?;
    let ci = gh.ci(&identity.source, run.pause)?;
    let Checked { raw, record, scan } = check_record(&paths.record, &paths.archive)?;
    let Some(fields) = record.as_object() else {
        return fail("Candidate record has an incorrect format.");
    };
    record_matches(fields, &identity, &producer, ci)?;
    artifact(&gh.api(&artifacts_path(ci))?, &identity, ci)?;
    for path in [&paths.archive, &paths.record] {
        let verified = gh.attestation(&attestation_args(
            &path.to_string_lossy(),
            &identity.source,
            &producer,
        ))?;
        attestation(&verified, &identity.source, &producer)?;
    }
    current_main(&gh, &identity)?;

    let source = FileSource {
        path: paths.archive.clone(),
    };
    let oci = &record["oci"];
    let manifest_digest =
        Digest::parse(oci["platform_manifest_digest"].as_str().unwrap_or_default())?;
    let manifest = read_manifest(
        &source,
        &scan,
        &manifest_digest,
        oci["platform_manifest_size_bytes"]
            .as_u64()
            .unwrap_or_default(),
    )?;
    let planned = blobs(&parse(&manifest, "OCI manifest")?)?;
    if planned.iter().any(|blob| {
        scan.blobs
            .get(blob.digest.hex())
            .is_none_or(|entry| entry.size != blob.size)
    }) {
        return fail("OCI blob descriptor size is incorrect.");
    }
    if let Some(parent) = paths.result.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| CiError::new(format!("{}: {error}", parent.display())))?;
    }
    let registry = Registry::login(run.transport, &actor, &run.token)?;
    upload(&registry, &source, &scan, &planned)?;
    registry.push_manifest(manifest_digest.as_str(), &manifest)?;
    recheck(&registry, &planned)?;
    current_main(&gh, &identity)?;

    let evidence = encode(&Published {
        record: fields,
        record_sha256: &sha256_hex(&raw),
        identity: &identity,
        ci,
        oci: &Value::clone(oci),
    })?;
    output::write_private(&paths.result, &evidence)?;
    Ok(PASS.to_owned())
}
