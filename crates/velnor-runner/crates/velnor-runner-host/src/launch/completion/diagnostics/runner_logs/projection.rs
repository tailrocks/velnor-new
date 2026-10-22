use std::io::{Cursor, Read};

use serde::Serialize;
use tar::{Archive, EntryType};

use crate::error::HostError;

use super::{MAX_ARCHIVE_BYTES, RunnerLogAvailability};

#[path = "projection/paths.rs"]
mod paths;
#[path = "projection/trace.rs"]
mod trace;
use paths::{ArchiveDirectory, auxiliary_file_name, directory_kind, relative_archive_path};
use trace::{Component, Event, parse_log_lines, parse_log_name};

const MAX_ARCHIVE_ENTRIES: usize = 256;
const MAX_LOG_FILES: usize = 8;
const MAX_AUXILIARY_FILES: usize = MAX_ARCHIVE_ENTRIES;
pub(super) const MAX_LOG_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_LOG_BYTES: usize = 32 * 1024 * 1024;
pub(super) const MAX_PROJECTION_BYTES: usize = 512 * 1024;

pub(super) fn valid_timestamp(value: &[u8]) -> bool {
    trace::valid_timestamp(value)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Projection {
    schema: u32,
    phase: &'static str,
    complete: bool,
    runner_log_present: bool,
    worker_log_present: bool,
    events: Vec<Event>,
    unrecognized_lines: u64,
    omitted_events: u64,
}

pub(super) struct SanitizedArchive {
    pub(super) bytes: Vec<u8>,
    pub(super) availability: RunnerLogAvailability,
}

#[cfg(test)]
pub(super) fn sanitize_archive(bytes: &[u8]) -> Result<Vec<u8>, HostError> {
    sanitize_archive_with_availability(bytes).map(|archive| archive.bytes)
}

pub(super) fn sanitize_archive_with_availability(
    bytes: &[u8],
) -> Result<SanitizedArchive, HostError> {
    if bytes.is_empty() || bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(HostError::Ownership);
    }
    let mut archive = Archive::new(Cursor::new(bytes));
    let mut state = ArchiveState::default();
    for entry in archive
        .entries()
        .map_err(|_| HostError::Ownership)?
        .raw(true)
    {
        state.consume(entry.map_err(|_| HostError::Ownership)?)?;
    }
    validate_terminator(archive.into_inner().position(), bytes)?;
    state.finish()
}

fn validate_terminator(position: u64, bytes: &[u8]) -> Result<(), HostError> {
    let position = usize::try_from(position).map_err(|_| HostError::Ownership)?;
    let trailing = bytes.get(position..).ok_or(HostError::Ownership)?;
    if trailing.len() < 512 || trailing.iter().any(|byte| *byte != 0) {
        return Err(HostError::Ownership);
    }
    Ok(())
}

#[derive(Default)]
struct ArchiveState {
    seen: Vec<(Component, String)>,
    events: Vec<Event>,
    total_bytes: usize,
    entries: usize,
    unrecognized_lines: u64,
    omitted_events: u64,
    seen_directories: Vec<ArchiveDirectory>,
    saw_runner: bool,
    saw_worker: bool,
    auxiliary_files: Vec<Vec<u8>>,
}

impl ArchiveState {
    fn consume<R: Read>(&mut self, entry: tar::Entry<'_, R>) -> Result<(), HostError> {
        self.entries += 1;
        if self.entries > MAX_ARCHIVE_ENTRIES {
            return Err(HostError::Ownership);
        }
        if entry.header().entry_type().is_dir() {
            let path = entry.path_bytes();
            return self.consume_directory(path.as_ref(), entry.size());
        }
        if entry.header().entry_type() != EntryType::Regular {
            return Err(HostError::Ownership);
        }
        self.consume_regular(entry)
    }

    fn consume_directory(&mut self, path: &[u8], size: u64) -> Result<(), HostError> {
        if size != 0 {
            return Err(HostError::Ownership);
        }
        let directory = directory_kind(path)?;
        if self.seen_directories.contains(&directory) {
            return Err(HostError::Ownership);
        }
        self.seen_directories.push(directory);
        Ok(())
    }

    fn consume_regular<R: Read>(&mut self, entry: tar::Entry<'_, R>) -> Result<(), HostError> {
        let path = entry.path_bytes().to_vec();
        let normalized_path = relative_archive_path(&path);
        if auxiliary_file_name(normalized_path)? {
            return self.consume_auxiliary(entry, normalized_path);
        }
        let (component, timestamp) = parse_log_name(&path)?;
        if self
            .seen
            .iter()
            .any(|existing| existing.0 == component && existing.1 == timestamp)
            || self.seen.len() >= MAX_LOG_FILES
        {
            return Err(HostError::Ownership);
        }
        self.seen.push((component, timestamp));
        let contents = read_log_entry(entry, &mut self.total_bytes)?;
        match component {
            Component::Runner => self.saw_runner = true,
            Component::Worker => self.saw_worker = true,
        }
        parse_log_lines(
            &contents,
            component,
            &mut self.events,
            &mut self.unrecognized_lines,
            &mut self.omitted_events,
        );
        Ok(())
    }

    fn consume_auxiliary<R: Read>(
        &mut self,
        entry: tar::Entry<'_, R>,
        path: &[u8],
    ) -> Result<(), HostError> {
        let size = usize::try_from(entry.size()).map_err(|_| HostError::Ownership)?;
        if size > MAX_ARCHIVE_BYTES
            || self.auxiliary_files.len() >= MAX_AUXILIARY_FILES
            || self.auxiliary_files.iter().any(|existing| existing == path)
        {
            return Err(HostError::Ownership);
        }
        self.auxiliary_files.push(path.to_vec());
        discard_auxiliary_entry(entry, size)
    }

    fn finish(self) -> Result<SanitizedArchive, HostError> {
        if self.seen.is_empty() && !self.seen_directories.contains(&ArchiveDirectory::Root) {
            return Err(HostError::Ownership);
        }
        let availability = match (self.saw_runner, self.saw_worker) {
            (false, false) => RunnerLogAvailability::None,
            (true, false) => RunnerLogAvailability::RunnerOnly,
            (false, true) => RunnerLogAvailability::WorkerOnly,
            (true, true) => RunnerLogAvailability::RunnerAndWorker,
        };
        let projection = Projection {
            schema: 1,
            phase: "runner_trace",
            complete: self.omitted_events == 0,
            runner_log_present: self.saw_runner,
            worker_log_present: self.saw_worker,
            events: self.events,
            unrecognized_lines: self.unrecognized_lines,
            omitted_events: self.omitted_events,
        };
        let output = serde_json::to_vec(&projection).map_err(|_| HostError::Journal)?;
        if output.len() > MAX_PROJECTION_BYTES {
            return Err(HostError::Ownership);
        }
        Ok(SanitizedArchive {
            bytes: output,
            availability,
        })
    }
}

fn read_log_entry<R: Read>(
    entry: tar::Entry<'_, R>,
    total_bytes: &mut usize,
) -> Result<Vec<u8>, HostError> {
    let size = usize::try_from(entry.size()).map_err(|_| HostError::Ownership)?;
    if size > MAX_LOG_FILE_BYTES || total_bytes.saturating_add(size) > MAX_TOTAL_LOG_BYTES {
        return Err(HostError::Ownership);
    }
    let mut contents = Vec::with_capacity(size);
    entry
        .take(u64::try_from(MAX_LOG_FILE_BYTES + 1).map_err(|_| HostError::Ownership)?)
        .read_to_end(&mut contents)
        .map_err(|_| HostError::Ownership)?;
    if contents.len() != size {
        return Err(HostError::Ownership);
    }
    *total_bytes = total_bytes.saturating_add(size);
    Ok(contents)
}

fn discard_auxiliary_entry<R: Read>(
    entry: tar::Entry<'_, R>,
    size: usize,
) -> Result<(), HostError> {
    let limit = u64::try_from(size.saturating_add(1)).map_err(|_| HostError::Ownership)?;
    let copied = std::io::copy(&mut entry.take(limit), &mut std::io::sink())
        .map_err(|_| HostError::Ownership)?;
    if copied != u64::try_from(size).map_err(|_| HostError::Ownership)? {
        return Err(HostError::Ownership);
    }
    Ok(())
}
