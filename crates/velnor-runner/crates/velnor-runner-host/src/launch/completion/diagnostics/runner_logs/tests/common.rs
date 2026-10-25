use tar::{Builder, EntryType, Header};

pub(super) const RUNNER_NAME: &str = "Runner_20261021-113000-utc.log";
pub(super) const WORKER_NAME: &str = "Worker_20261021-113001-utc.log";
pub(super) const TRACE: &[u8] =
    b"[2026-10-21 11:30:00Z INFO Runner.Worker] job canary-message-9e26\n";

pub(super) fn trace_at_size(size: usize) -> Vec<u8> {
    let mut contents = Vec::with_capacity(size);
    while contents.len().saturating_add(TRACE.len()) <= size {
        contents.extend_from_slice(TRACE);
    }
    contents.resize(size, b'\n');
    contents
}

pub(super) fn archive(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let mut builder = Builder::new(Vec::new());
    for (name, contents) in entries {
        let mut header = Header::new_gnu();
        header.set_size(u64::try_from(contents.len()).map_err(|error| error.to_string())?);
        header.set_mode(0o600);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *contents)
            .map_err(|error| error.to_string())?;
    }
    builder.into_inner().map_err(|error| error.to_string())
}

pub(super) fn valid_archive() -> Result<Vec<u8>, String> {
    archive(&[(RUNNER_NAME, TRACE), (WORKER_NAME, TRACE)])
}

pub(super) fn directory_archive(name: &str) -> Result<Vec<u8>, String> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_size(0);
    header.set_mode(0o700);
    header.set_path(name).map_err(|error| error.to_string())?;
    header.set_cksum();
    let mut builder = Builder::new(Vec::new());
    builder
        .append(&header, &[][..])
        .map_err(|error| error.to_string())?;
    builder.into_inner().map_err(|error| error.to_string())
}

pub(super) fn pax_archive() -> Result<Vec<u8>, String> {
    let mut builder = Builder::new(Vec::new());
    let pax_record = b"10 path=x\n";
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::XHeader);
    header.set_size(u64::try_from(pax_record.len()).map_err(|error| error.to_string())?);
    header.set_mode(0o600);
    header
        .set_path("PaxHeaders.0/log")
        .map_err(|error| error.to_string())?;
    header.set_cksum();
    builder
        .append(&header, &pax_record[..])
        .map_err(|error| error.to_string())?;
    for (name, contents) in [(RUNNER_NAME, TRACE), (WORKER_NAME, TRACE)] {
        let mut header = Header::new_gnu();
        header.set_size(u64::try_from(contents.len()).map_err(|error| error.to_string())?);
        header.set_mode(0o600);
        header.set_path(name).map_err(|error| error.to_string())?;
        header.set_cksum();
        builder
            .append(&header, contents)
            .map_err(|error| error.to_string())?;
    }
    builder.into_inner().map_err(|error| error.to_string())
}

pub(super) fn too_many_entries_archive() -> Result<Vec<u8>, String> {
    let mut builder = Builder::new(Vec::new());
    for name in ["_diag/", "_diag/pages/"] {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o700);
        header.set_path(name).map_err(|error| error.to_string())?;
        header.set_cksum();
        builder
            .append(&header, &[][..])
            .map_err(|error| error.to_string())?;
    }
    for page in 1..=255 {
        let name = format!(
            "_diag/pages/00000000-0000-0000-0000-000000000000_00000000-0000-0000-0000-000000000001_{page}.log"
        );
        let mut header = Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o600);
        header.set_path(name).map_err(|error| error.to_string())?;
        header.set_cksum();
        builder
            .append(&header, &[][..])
            .map_err(|error| error.to_string())?;
    }
    builder.into_inner().map_err(|error| error.to_string())
}

pub(super) fn archive_with_paging_logs() -> Result<Vec<u8>, String> {
    let mut builder = Builder::new(Vec::new());
    for name in ["_diag/", "_diag/pages/", "_diag/blocks/"] {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o700);
        header.set_path(name).map_err(|error| error.to_string())?;
        header.set_cksum();
        builder
            .append(&header, &[][..])
            .map_err(|error| error.to_string())?;
    }
    for (name, contents) in [
        (RUNNER_NAME, TRACE),
        (WORKER_NAME, TRACE),
        (
            "_diag/pages/00000000-0000-0000-0000-000000000000_00000000-0000-0000-0000-000000000001_1.log",
            b"private job page canary",
        ),
        (
            "_diag/blocks/00000000-0000-0000-0000-000000000000_00000000-0000-0000-0000-000000000001.1",
            b"private results block canary",
        ),
    ] {
        let mut header = Header::new_gnu();
        header.set_size(u64::try_from(contents.len()).map_err(|error| error.to_string())?);
        header.set_mode(0o600);
        header.set_path(name).map_err(|error| error.to_string())?;
        header.set_cksum();
        builder
            .append(&header, contents)
            .map_err(|error| error.to_string())?;
    }
    builder.into_inner().map_err(|error| error.to_string())
}

pub(super) fn forged_path_archive(path: &[u8]) -> Result<Vec<u8>, String> {
    let mut header = Header::new_gnu();
    header.set_size(u64::try_from(TRACE.len()).map_err(|error| error.to_string())?);
    header.set_mode(0o600);
    header
        .set_path("placeholder.log")
        .map_err(|error| error.to_string())?;
    let name = &mut header.as_mut_bytes()[..100];
    name.fill(0);
    name[..path.len()].copy_from_slice(path);
    header.set_cksum();
    let mut builder = Builder::new(Vec::new());
    builder
        .append(&header, TRACE)
        .map_err(|error| error.to_string())?;
    let mut worker = Header::new_gnu();
    worker.set_size(0);
    worker.set_mode(0o600);
    worker
        .set_path(WORKER_NAME)
        .map_err(|error| error.to_string())?;
    worker.set_cksum();
    builder
        .append(&worker, &[][..])
        .map_err(|error| error.to_string())?;
    builder.into_inner().map_err(|error| error.to_string())
}
