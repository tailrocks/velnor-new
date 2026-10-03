//! Attribution from actual Cargo manifest/source inputs and public metadata.
//! Labels, output filename patterns and cached root probes supply no authority.

use mbx_cache_core::{PackageOrigin, UnitIdentity, UnitProvenance};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Capture native inputs before any measured process. A missing source remains
/// explicit: manifest context can still identify a build script's native work.
pub(crate) fn identity(arguments: &[OsString], source: Option<&Path>) -> Option<UnitIdentity> {
    native_identity(arguments, source, UnitProvenance::CargoTarget)
}

/// The actual native child belongs to the invoking Cargo package context.
/// Its source location is retained, with source ownership explicitly unknown.
pub(crate) fn package_context(source: Option<&Path>) -> Option<UnitIdentity> {
    native_identity(&[], source, UnitProvenance::CargoPackageContext)
}

fn native_identity(
    arguments: &[OsString],
    source: Option<&Path>,
    provenance: UnitProvenance,
) -> Option<UnitIdentity> {
    let working_dir = std::env::current_dir().ok()?;
    let manifest = manifest_evidence(
        std::env::var_os("CARGO_MANIFEST_DIR"),
        std::env::var_os("CARGO_MANIFEST_PATH"),
        &working_dir,
    );
    let expanded = mbx_cache_rustc::RustcInvocation::expand_arguments(arguments).ok();
    let arguments = expanded.as_deref().unwrap_or(arguments);
    let parsed = mbx_cache_rustc::RustcInvocation::parse_with(
        arguments,
        mbx_cache_rustc::ParseOptions::caching_native_links(true),
    )
    .ok();
    let source = source
        .or_else(|| parsed.as_ref().map(|invocation| invocation.source()))
        .and_then(|path| observed_path(path, &working_dir));
    let hash = extra_filename(arguments);
    if manifest.is_none() && source.is_none() && hash.is_none() {
        return None;
    }
    Some(UnitIdentity {
        cargo_unit_id: hash,
        manifest_path: manifest,
        source_path: source,
        package_id: None,
        origin: PackageOrigin::Unknown,
        source_origin: PackageOrigin::Unknown,
        provenance,
    })
}

fn manifest_evidence(
    directory: Option<OsString>,
    manifest: Option<OsString>,
    cwd: &Path,
) -> Option<PathBuf> {
    let from_directory = directory.map(|root| PathBuf::from(root).join("Cargo.toml"));
    let from_manifest = manifest.map(PathBuf::from);
    if from_directory
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
        || from_manifest
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
    {
        return None;
    }
    match (from_directory, from_manifest) {
        (Some(directory), Some(manifest)) => {
            let directory = observed_path(&directory, cwd)?;
            (directory == observed_path(&manifest, cwd)?).then_some(directory)
        }
        (Some(manifest), None) | (None, Some(manifest)) => observed_path(&manifest, cwd),
        (None, None) => None,
    }
}

/// Observe only the hash literally supplied in compiler arguments. Directory
/// spellings and crate labels are not unit authority. Conflicts remain unknown.
fn extra_filename(arguments: &[OsString]) -> Option<String> {
    let mut result = None;
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let argument = argument.to_str()?;
        let value = match argument {
            "-C" | "--codegen" => arguments.next()?.to_str()?,
            _ => match argument
                .strip_prefix("-C")
                .or_else(|| argument.strip_prefix("--codegen="))
            {
                Some(value) => value,
                None => continue,
            },
        };
        let Some(hash) = value.strip_prefix("extra-filename=-") else {
            continue;
        };
        if hash.is_empty() || hash.len() > 256 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        if result.as_deref().is_some_and(|existing| existing != hash) {
            return None;
        }
        result = Some(hash.to_string());
    }
    result
}

/// Preserve actual existing filesystem identity, with bounded absolute paths.
pub(crate) fn observed_path(path: &Path, cwd: &Path) -> Option<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let path = path.canonicalize().ok()?;
    (path.to_str().is_some_and(|value| value.len() <= 4096)).then_some(path)
}

#[cfg(test)]
#[path = "unit_attribution_tests.rs"]
mod tests;
