use crate::session::completed_report::{SessionIdentity, prepare_directory, valid_uuid};
use eyre::{Result, bail};
use mbx_cache_core::AdapterKind;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AdmissionKind {
    Invocation,
    Process,
    Output,
    BuildScriptProvision,
    CcSelection,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum TerminalOutcome {
    Acknowledged { event_sha256: String },
    Failed { reason: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct EntryRecord {
    pub session_id: String,
    pub root_session_id: String,
    pub event_id: String,
    pub adapter: AdapterKind,
    pub kind: AdmissionKind,
}

#[derive(Serialize, Deserialize)]
struct LedgerIdentity {
    session_id: String,
    root_session_id: String,
}

/// Created only by the native owning session, retained until publication.
pub(crate) struct AdmissionOwner {
    pub(super) directory: PathBuf,
    pub(super) identity: SessionIdentity,
}

impl AdmissionOwner {
    pub(crate) fn open(report_directory: &Path, identity: &SessionIdentity) -> Result<Self> {
        identity.validate()?;
        if !cfg!(unix) || !cfg!(feature = "owned-cache-transport") {
            bail!("native admission ledger requires the owned Unix source");
        }
        prepare_directory(report_directory)?;
        let directory = directory(report_directory, &identity.session_id);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        prepare_directory(&directory)?;
        let _lock = lock(&directory)?;
        publish(
            &directory,
            "identity.json",
            &LedgerIdentity {
                session_id: identity.session_id.clone(),
                root_session_id: identity.root_session_id.clone(),
            },
        )?;
        Ok(Self {
            directory,
            identity: identity.clone(),
        })
    }
}

/// An actual accepted native event. Dropping it never invents a terminal.
pub(crate) struct AdmissionEntry {
    directory: PathBuf,
    event_id: String,
}

impl AdmissionEntry {
    pub(super) fn from_environment(
        adapter: AdapterKind,
        kind: AdmissionKind,
    ) -> Result<Option<Self>> {
        use crate::session::completed_report::{ROOT_SESSION_ID_ENV, SESSION_ID_ENV};
        let Some(directory) = std::env::var_os("MBX_STATS_REPORT_DIR") else {
            return Ok(None);
        };
        Self::enroll(
            Path::new(&directory),
            &std::env::var(SESSION_ID_ENV)?,
            &std::env::var(ROOT_SESSION_ID_ENV)?,
            &hex::encode(rand::random::<[u8; 16]>()),
            adapter,
            kind,
        )
    }

    pub(super) fn evidence<T: Serialize>(self, value: &T) -> Result<()> {
        use sha2::{Digest, Sha256};
        let digest = hex::encode(Sha256::digest(serde_json::to_vec(value)?));
        {
            let _lock = lock(&self.directory)?;
            if self.directory.join("closed.json").exists() {
                bail!("admission closed before dynamic evidence");
            }
            publish(
                &self.directory,
                &format!("{}.evidence.json", self.event_id),
                value,
            )?;
        }
        self.finish(TerminalOutcome::Acknowledged {
            event_sha256: digest,
        })
    }

    pub(crate) fn enroll(
        report_directory: &Path,
        session_id: &str,
        root_session_id: &str,
        event_id: &str,
        adapter: AdapterKind,
        kind: AdmissionKind,
    ) -> Result<Option<Self>> {
        validate_id(session_id, root_session_id, event_id)?;
        let directory = directory(report_directory, session_id);
        if !directory.is_dir() {
            bail!("owning native admission ledger is unavailable");
        }
        let _lock = lock(&directory)?;
        let identity: LedgerIdentity =
            serde_json::from_slice(&read(&directory.join("identity.json"))?)?;
        if identity.session_id != session_id || identity.root_session_id != root_session_id {
            bail!("native admission identity mismatch");
        }
        if directory.join("closed.json").exists() {
            return Ok(None);
        }
        let record = EntryRecord {
            session_id: session_id.into(),
            root_session_id: root_session_id.into(),
            event_id: event_id.into(),
            adapter,
            kind,
        };
        publish(&directory, &format!("{event_id}.accepted.json"), &record)?;
        Ok(Some(Self {
            directory,
            event_id: event_id.into(),
        }))
    }

    pub(crate) fn finish(self, outcome: TerminalOutcome) -> Result<()> {
        if let TerminalOutcome::Failed { reason } = &outcome {
            if reason.is_empty() || reason.len() > 1024 {
                bail!("invalid admission failure reason");
            }
        }
        if let TerminalOutcome::Acknowledged { event_sha256 } = &outcome {
            if !valid_digest(event_sha256) {
                bail!("invalid admitted terminal digest");
            }
        }
        let _lock = lock(&self.directory)?;
        if self.directory.join("closed.json").exists() {
            bail!("admission already closed with this entry outstanding");
        }
        publish(
            &self.directory,
            &format!("{}.terminal.json", self.event_id),
            &outcome,
        )
    }
}

fn directory(report: &Path, session: &str) -> PathBuf {
    report.join(format!(".mbx-admissions-{session}"))
}

pub(super) fn validate_id(session: &str, root: &str, event: &str) -> Result<()> {
    if !valid_uuid(session)
        || !valid_uuid(root)
        || event.len() != 32
        || !event.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("invalid native admission identifier");
    }
    Ok(())
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn read(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let before = std::fs::symlink_metadata(path)?;
    if !before.is_file() || before.len() > 16 * 1024 * 1024 {
        bail!("invalid native admission record file");
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(path)?;
    let opened = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != opened.dev()
            || before.ino() != opened.ino()
            || opened.nlink() != 1
            || opened.mode() & 0o777 != 0o400
        {
            bail!("native admission record is not immutable private evidence");
        }
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != opened.len() || bytes.len() > 16 * 1024 * 1024 {
        bail!("native admission record changed or exceeds bound");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let after = file.metadata()?;
        let current = std::fs::symlink_metadata(path)?;
        if after.dev() != opened.dev()
            || after.ino() != opened.ino()
            || after.size() != opened.size()
            || after.mtime() != opened.mtime()
            || after.mtime_nsec() != opened.mtime_nsec()
            || after.ctime() != opened.ctime()
            || after.ctime_nsec() != opened.ctime_nsec()
            || after.dev() != current.dev()
            || after.ino() != current.ino()
            || current.mode() != opened.mode()
        {
            bail!("native admission record changed during read");
        }
    }
    Ok(bytes)
}

pub(super) fn lock(directory: &Path) -> Result<fslock::LockFile> {
    let mut lock = fslock::LockFile::open(&directory.join("ledger.lock"))?;
    lock.lock()?;
    Ok(lock)
}

pub(super) fn publish<T: Serialize>(directory: &Path, name: &str, value: &T) -> Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > 16 * 1024 * 1024 {
        bail!("native admission publication exceeds bound");
    }
    file.write_all(&bytes)?;
    file.as_file_mut().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o400))?;
    }
    file.persist_noclobber(directory.join(name))
        .map_err(|error| error.error)?;
    std::fs::File::open(directory)?.sync_all()?;
    Ok(())
}
