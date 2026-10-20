use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tough::{
    ExpirationEnforcement, IntoVec, Limits, Repository, RepositoryLoader, TargetName, Transport,
};
use url::Url;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

use crate::root_chain::{RootIdentity, VerifiedRootChain, verify_composite_chain};

const CURRENT_FILE: &str = "CURRENT";
const LOCK_FILE: &str = "LOCK";
const GENERATIONS_DIR: &str = "generations";
const STATE_FILE: &str = "root-state.json";
const MANIFEST_FILE: &str = "manifest.json";
const TUF_DIR: &str = "tuf";
const MAX_STATE_FILE_BYTES: u64 = 1024 * 1024;
const MAX_METADATA_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CAPTURED_ROOT_RESPONSES: usize = 64;
const MAX_CAPTURED_ROOT_BYTES: usize = 64 * 1024 * 1024;
const MAX_CAPTURED_SINGLE_ROOT_BYTES: usize = 1024 * 1024;
const LOCK_WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Default)]
pub(crate) struct RootResponseCapture {
    bodies: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl RootResponseCapture {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record(&self, url: &Url, bytes: &[u8]) -> Result<(), String> {
        let Some(name) = url.path_segments().and_then(Iterator::last) else {
            return Ok(());
        };
        let Some(version) = name.strip_suffix(".root.json") else {
            return Ok(());
        };
        if version.is_empty() || !version.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok(());
        }
        let mut bodies = self
            .bodies
            .lock()
            .map_err(|_| "root response capture lock poisoned".to_owned())?;
        let total = bodies
            .iter()
            .map(Vec::len)
            .fold(0_usize, usize::saturating_add);
        if bytes.len() > MAX_CAPTURED_SINGLE_ROOT_BYTES
            || bodies.len() >= MAX_CAPTURED_ROOT_RESPONSES
            || total.saturating_add(bytes.len()) > MAX_CAPTURED_ROOT_BYTES
        {
            return Err("captured TUF root chain exceeds byte or count bound".to_owned());
        }
        bodies.push(bytes.to_vec());
        Ok(())
    }

    pub(crate) fn take(&self) -> Result<Vec<Vec<u8>>, String> {
        let mut bodies = self
            .bodies
            .lock()
            .map_err(|_| "root response capture lock poisoned".to_owned())?;
        Ok(std::mem::take(&mut *bodies))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BootstrapMigration {
    pub(crate) from_sha256: String,
    pub(crate) from_root: RootIdentity,
    pub(crate) to_sha256: String,
    pub(crate) to_root: RootIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootHighWater {
    schema: u8,
    bootstrap_sha256: String,
    bootstrap_version: u64,
    bootstrap_signed_sha256: String,
    root_version: u64,
    root_signed_sha256: String,
    tuf_files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDigest {
    size: u64,
    sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerationManifest {
    schema: u8,
    generation: String,
    files: BTreeMap<String, FileDigest>,
}

pub(crate) struct TufRefresh<V> {
    #[cfg(test)]
    pub(crate) repository: Repository,
    #[cfg(test)]
    pub(crate) root_chain: VerifiedRootChain,
    pub(crate) target_value: V,
}

pub(crate) struct TufRefreshRequest<'a, T, F> {
    pub(crate) bootstrap: &'a [u8],
    pub(crate) migration: Option<&'a BootstrapMigration>,
    pub(crate) metadata_url: Url,
    pub(crate) targets_url: Url,
    pub(crate) target_name: Option<&'a TargetName>,
    pub(crate) transport: T,
    pub(crate) capture: &'a RootResponseCapture,
    pub(crate) validate_target: F,
}

pub(crate) struct TufCache {
    pub(crate) root: PathBuf,
}

include!("tuf_state/cache_impl.inc.rs");
include!("tuf_state/cache_storage.inc.rs");

include!("tuf_state/generation.inc.rs");
include!("tuf_state/lock.inc.rs");
include!("tuf_state/path.inc.rs");
include!("tuf_state/files.inc.rs");

#[cfg(test)]
#[path = "tuf_state_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tuf_state_process_tests.rs"]
mod process_tests;
