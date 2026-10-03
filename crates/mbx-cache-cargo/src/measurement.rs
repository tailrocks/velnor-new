//! Fresh public Cargo package authority for explicitly requested measurement.
//! This never uses the roots cache, fetches sources, or plans executable tasks.

use mbx_cache_core::PackageOrigin;
use serde::Deserialize;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PACKAGES: usize = 4096;
const MAX_TARGETS: usize = 256;
const MAX_TEXT: usize = 4096;
static LIVE_READERS: AtomicUsize = AtomicUsize::new(0);

struct ReaderPermit;
impl ReaderPermit {
    fn acquire() -> Option<Self> {
        LIVE_READERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 2).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for ReaderPermit {
    fn drop(&mut self) {
        LIVE_READERS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A package described by Cargo's public metadata protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasurementPackage {
    /// Cargo's opaque package identity, retained verbatim.
    pub package_id: String,
    /// Actual resolved package manifest.
    pub manifest_path: PathBuf,
    /// Actual target source files reported by Cargo.
    pub sources: Vec<PathBuf>,
    /// Workspace membership or source authority, read from metadata fields.
    pub origin: PackageOrigin,
}

/// Whether fresh package authority was available for this attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataObservation {
    /// Cargo exited successfully and supplied bounded valid package authority.
    Observed,
    /// Cargo could not supply fresh authority without fetching or unlocking.
    Unavailable,
    /// The supported selector or public output exceeded its admitted bounds.
    Unsupported,
}

/// A fresh measurement setup observation, separate from workload wall.
#[derive(Debug, Clone)]
pub struct MeasurementMetadata {
    /// Authority observed in this single metadata response; empty on failure.
    pub packages: Vec<MeasurementPackage>,
    /// Explicit status; empty packages never imply an empty compilation graph.
    pub observation: MetadataObservation,
    /// Setup interval, including spawn, read and wait, in nanoseconds.
    pub observed_wall_ns: u64,
}

/// Observe fresh full public metadata using the selected actual Cargo binary.
/// Call only when a measurement report was requested. Failure preserves the
/// owning workload and leaves ownership unknown. No diagnostics/argv escape.
pub fn measurement_metadata(
    cargo: &OsStr,
    arguments: &[String],
    working_dir: &Path,
) -> MeasurementMetadata {
    let started = Instant::now();
    let mut observation = MetadataObservation::Unavailable;
    let packages = match Path::new(cargo)
        .is_absolute()
        .then(|| measurement_arguments(arguments))
        .flatten()
    {
        Some(arguments) => metadata_output(cargo, &arguments, working_dir)
            .and_then(|bytes| parse_metadata(&bytes))
            .inspect(|_| observation = MetadataObservation::Observed)
            .unwrap_or_default(),
        None => {
            observation = MetadataObservation::Unsupported;
            Vec::new()
        }
    };
    MeasurementMetadata {
        packages,
        observation,
        observed_wall_ns: started.elapsed().as_nanos().try_into().unwrap_or(u64::MAX),
    }
}

fn metadata_output(cargo: &OsStr, arguments: &[String], cwd: &Path) -> Option<Vec<u8>> {
    metadata_output_with_timeout(cargo, arguments, cwd, Duration::from_secs(30))
}

fn metadata_output_with_timeout(
    cargo: &OsStr,
    arguments: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Option<Vec<u8>> {
    // A descendant retaining stdout cannot accumulate unbounded observers.
    let permit = Arc::new(ReaderPermit::acquire()?);
    let deadline = Instant::now().checked_add(timeout)?;
    let mut child = Command::new(cargo)
        .args(arguments)
        .current_dir(cwd)
        // Cargo --offline does not constrain an installed Rustup proxy's
        // toolchain installation. Preserve its native selection, forbid install.
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let Some(stdout) = child.stdout.take() else {
        reap_unavailable(child, Arc::clone(&permit));
        return None;
    };
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let read_permit = Arc::clone(&permit);
    let reader = std::thread::Builder::new().spawn(move || {
        let _permit = read_permit;
        let mut bytes = Vec::new();
        let result = stdout
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()
            .and_then(|_| (bytes.len() as u64 <= MAX_BYTES).then_some(bytes));
        if send.send(result).is_err() { /* The owning deadline expired. */ }
    });
    if reader.is_err() {
        reap_unavailable(child, Arc::clone(&permit));
        return None;
    }
    let bytes = receive
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()
        .flatten();
    if bytes.is_none() {
        reap_unavailable(child, Arc::clone(&permit));
        return None;
    }
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then_some(bytes).flatten(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => {
                reap_unavailable(child, Arc::clone(&permit));
                return None;
            }
        }
    }
}

fn reap_unavailable(mut child: std::process::Child, permit: Arc<ReaderPermit>) {
    if let Err(error) = child.kill() {
        log_unavailable(error);
    }
    // A broken wait cannot delay the owning workload indefinitely.
    if std::thread::Builder::new()
        .spawn(move || {
            let _permit = permit;
            if child.wait().is_err() { /* Terminal observation unavailable. */ }
        })
        .is_err()
    { /* Kill attempted; terminal observation unavailable. */ }
}

fn log_unavailable(_error: std::io::Error) {
    // Failure remains a typed unavailable observation, without public paths.
}

fn measurement_arguments(arguments: &[String]) -> Option<Vec<String>> {
    // Reuse the owner's manifest/config/global parser; exclude harness args.
    let arguments = arguments.split(|argument| argument == "--").next()?;
    // The selected actual binary must already own any rustup toolchain
    // selection. A proxy selector cannot be silently dropped here.
    if arguments.iter().any(|argument| argument.starts_with('+')) {
        return None;
    }
    if super::path_install_dir(arguments, Path::new(".")).is_some() {
        // Installation's source/cwd rebasing needs a separate supported owner.
        return None;
    }
    let mut result = super::forwarded_flags(arguments, &["-C", "--directory", "--config", "-Z"]);
    // Preserve global ordering, then append the owner's manifest/toggle suffix.
    let owner = super::probe_arguments(arguments);
    let metadata = owner.iter().position(|argument| argument == "metadata")?;
    result.extend_from_slice(&owner[metadata..]);
    result.retain(|argument| argument != "--no-deps");
    for flag in ["--locked", "--offline"] {
        if !result.iter().any(|argument| argument == flag) {
            result.push(flag.into());
        }
    }
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(flag, value)| {
                (flag, Some(value))
            });
        match flag {
            "--features" | "-F" | "--target" => {
                let value = inline.or_else(|| arguments.next().map(String::as_str))?;
                if value.is_empty() || value.len() > MAX_TEXT {
                    return None;
                }
                result.extend([
                    if flag == "--target" {
                        "--filter-platform"
                    } else {
                        "--features"
                    }
                    .into(),
                    value.into(),
                ]);
            }
            "--all-features" | "--no-default-features" => result.push(flag.into()),
            value if value.starts_with("-F") && value.len() > 2 => {
                if value.len() > MAX_TEXT + 2 {
                    return None;
                }
                result.extend(["--features".into(), value[2..].into()]);
            }
            // A value belonging to another flag cannot become a feature flag.
            "--config" | "--manifest-path" | "-Z" | "-C" | "--directory" | "-p" | "--package"
            | "-j" | "--jobs" | "--exclude" | "--bin" | "--example" | "--test" | "--bench"
            | "--profile" | "--target-dir" | "--message-format"
                if inline.is_none() =>
            {
                arguments.next()?;
            }
            _ => {}
        }
    }
    (result.len() <= 4096 && result.iter().all(|argument| argument.len() <= MAX_TEXT))
        .then_some(result)
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}
#[derive(Deserialize)]
struct Package {
    id: String,
    source: serde_json::Value,
    manifest_path: PathBuf,
    targets: Vec<Target>,
}
#[derive(Deserialize)]
struct Target {
    src_path: PathBuf,
}

fn parse_metadata(bytes: &[u8]) -> Option<Vec<MeasurementPackage>> {
    let metadata: Metadata = serde_json::from_slice(bytes).ok()?;
    if metadata.packages.len() > MAX_PACKAGES || metadata.workspace_members.len() > MAX_PACKAGES {
        return None;
    }
    let mut packages = Vec::with_capacity(metadata.packages.len());
    let mut ids = std::collections::BTreeSet::new();
    for package in metadata.packages {
        let source = match package.source {
            serde_json::Value::Null => None,
            serde_json::Value::String(source) => Some(source),
            _ => return None,
        };
        if package.id.is_empty()
            || package.id.len() > MAX_TEXT
            || source
                .as_ref()
                .is_some_and(|source| source.len() > MAX_TEXT)
            || !ids.insert(package.id.clone())
            || !valid_path(&package.manifest_path)
            || package.targets.len() > MAX_TARGETS
            || package
                .targets
                .iter()
                .any(|target| !valid_path(&target.src_path))
        {
            return None;
        }
        let origin = if metadata.workspace_members.contains(&package.id) {
            PackageOrigin::Workspace
        } else {
            match source.as_deref() {
                Some(source)
                    if source.starts_with("registry+") || source.starts_with("sparse+") =>
                {
                    PackageOrigin::Registry
                }
                Some(source) if source.starts_with("git+") => PackageOrigin::Git,
                None => PackageOrigin::Path,
                Some(_) => PackageOrigin::Unknown,
            }
        };
        packages.push(MeasurementPackage {
            package_id: package.id,
            manifest_path: package.manifest_path,
            sources: package
                .targets
                .into_iter()
                .map(|target| target.src_path)
                .collect(),
            origin,
        });
    }
    metadata
        .workspace_members
        .iter()
        .all(|id| ids.contains(id))
        .then_some(packages)
}

fn valid_path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|value| value.len() <= MAX_TEXT)
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

#[cfg(test)]
#[path = "measurement_tests.rs"]
mod tests;
