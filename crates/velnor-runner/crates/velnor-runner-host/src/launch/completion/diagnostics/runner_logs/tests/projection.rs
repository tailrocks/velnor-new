use crate::error::HostError;
use tar::{Builder, EntryType, Header};

use super::super::projection::{MAX_LOG_FILE_BYTES, sanitize_archive, valid_timestamp};
use super::common::{
    RUNNER_NAME, TRACE, WORKER_NAME, archive, archive_with_paging_logs, directory_archive,
    forged_path_archive, pax_archive, too_many_entries_archive, valid_archive,
};

#[test]
fn projection_discards_message_and_header_source_text() -> Result<(), String> {
    let bytes = valid_archive()?;
    let projection = sanitize_archive(&bytes).map_err(|error| format!("{error:?}"))?;
    let text = String::from_utf8(projection).map_err(|error| error.to_string())?;
    if text.contains("canary-message-9e26") || text.contains("Runner.Worker") {
        return Err("untrusted trace text reached the durable projection".to_owned());
    }
    if !text.contains("\"component\":\"runner\"")
        || !text.contains("\"component\":\"worker\"")
        || !text.contains("\"category\":\"information\"")
        || !text.contains("\"complete\":true")
    {
        return Err("closed trace projection omitted expected typed fields".to_owned());
    }
    Ok(())
}

#[test]
fn canary_payload_changes_do_not_change_the_projection() -> Result<(), String> {
    let benign = b"[2026-10-21 11:30:00Z INFO Runner.Worker] startup failed\n";
    let secret = b"[2026-10-21 11:30:00Z INFO Private.Source] https://user:token@example.invalid/job/42?key=abcd base64-QUJD\x1b[31m\n";
    let baseline = archive(&[(RUNNER_NAME, benign), (WORKER_NAME, benign)])?;
    let canary = archive(&[(RUNNER_NAME, secret), (WORKER_NAME, secret)])?;
    let baseline = sanitize_archive(&baseline).map_err(|error| format!("{error:?}"))?;
    let canary = sanitize_archive(&canary).map_err(|error| format!("{error:?}"))?;
    if baseline != canary {
        return Err("untrusted trace body affected persisted output".to_owned());
    }
    Ok(())
}

#[test]
fn unrecognized_lines_are_counted_without_retaining_their_contents() -> Result<(), String> {
    let runner = b"authorization canary-secret-7f9c\n";
    let worker = b"[2026-10-21 11:30:01Z WARN Worker.Listener] private payload\n";
    let bytes = archive(&[(RUNNER_NAME, runner), (WORKER_NAME, worker)])?;
    let projection = sanitize_archive(&bytes).map_err(|error| format!("{error:?}"))?;
    let text = String::from_utf8(projection).map_err(|error| error.to_string())?;
    if text.contains("canary-secret-7f9c") || text.contains("private payload") {
        return Err("untrusted trace body reached the durable projection".to_owned());
    }
    if !text.contains("\"unrecognized_lines\":1") {
        return Err("unrecognized line count was not preserved".to_owned());
    }
    Ok(())
}

#[test]
fn archive_rejects_path_aliases_links_and_duplicates() -> Result<(), String> {
    for bad_path in [
        b"../Runner_20261021-113000-utc.log".as_slice(),
        b"/Runner_20261021-113000-utc.log".as_slice(),
        b"sub/Runner_20261021-113000-utc.log".as_slice(),
    ] {
        let bytes = forged_path_archive(bad_path)?;
        if sanitize_archive(&bytes).is_ok() {
            return Err(format!("unsafe path was accepted: {bad_path:?}"));
        }
    }
    let duplicate = archive(&[(RUNNER_NAME, TRACE), (RUNNER_NAME, TRACE)])?;
    if sanitize_archive(&duplicate).is_ok() {
        return Err("duplicate archive entry was accepted".to_owned());
    }
    let runner_only = sanitize_archive(&archive(&[(RUNNER_NAME, TRACE)])?)
        .map_err(|error| format!("{error:?}"))?;
    let runner_only = String::from_utf8(runner_only).map_err(|error| error.to_string())?;
    if !runner_only.contains("\"runner_log_present\":true")
        || !runner_only.contains("\"worker_log_present\":false")
    {
        return Err("missing Worker log was not recorded as an absent category".to_owned());
    }
    let worker_only = sanitize_archive(&archive(&[(WORKER_NAME, TRACE)])?)
        .map_err(|error| format!("{error:?}"))?;
    let worker_only = String::from_utf8(worker_only).map_err(|error| error.to_string())?;
    if !worker_only.contains("\"runner_log_present\":false")
        || !worker_only.contains("\"worker_log_present\":true")
    {
        return Err("missing Runner log was not recorded as an absent category".to_owned());
    }
    let mut builder = Builder::new(Vec::new());
    let mut link = Header::new_gnu();
    link.set_entry_type(EntryType::symlink());
    link.set_size(0);
    link.set_mode(0o777);
    link.set_link_name("/etc/passwd")
        .map_err(|error| error.to_string())?;
    link.set_cksum();
    builder
        .append_data(&mut link, RUNNER_NAME, &[][..])
        .map_err(|error| error.to_string())?;
    let mut regular = Header::new_gnu();
    regular.set_size(0);
    regular.set_mode(0o600);
    regular.set_cksum();
    builder
        .append_data(&mut regular, WORKER_NAME, &[][..])
        .map_err(|error| error.to_string())?;
    let linked = builder.into_inner().map_err(|error| error.to_string())?;
    if sanitize_archive(&linked).is_ok() {
        return Err("symlink archive entry was accepted".to_owned());
    }
    Ok(())
}

#[test]
fn archive_accepts_observed_root_shapes_and_records_empty_categories() -> Result<(), String> {
    for root in [".", "_diag"] {
        let bytes = directory_archive(root)?;
        let projection = sanitize_archive(&bytes).map_err(|error| format!("{error:?}"))?;
        let projection = String::from_utf8(projection).map_err(|error| error.to_string())?;
        if !projection.contains("\"runner_log_present\":false")
            || !projection.contains("\"worker_log_present\":false")
            || !projection.contains("\"complete\":true")
        {
            return Err(format!(
                "empty {root:?} archive did not record absent categories"
            ));
        }
    }
    let rooted_files = archive(&[
        ("_diag/Runner_20261021-113000-utc.log", TRACE),
        ("./Worker_20261021-113001-utc.log", TRACE),
    ])?;
    let projection = sanitize_archive(&rooted_files).map_err(|error| format!("{error:?}"))?;
    if !String::from_utf8(projection)
        .map_err(|error| error.to_string())?
        .contains("\"worker_log_present\":true")
    {
        return Err("canonical _diag-prefixed log entries were not recognized".to_owned());
    }
    let duplicate_root = {
        let mut builder = Builder::new(Vec::new());
        for root in [".", "_diag"] {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o700);
            header.set_path(root).map_err(|error| error.to_string())?;
            header.set_cksum();
            builder
                .append(&header, &[][..])
                .map_err(|error| error.to_string())?;
        }
        builder.into_inner().map_err(|error| error.to_string())?
    };
    if sanitize_archive(&duplicate_root).is_ok() {
        return Err("multiple archive roots were accepted".to_owned());
    }
    let duplicate_file_alias = archive(&[
        (RUNNER_NAME, TRACE),
        ("_diag/Runner_20261021-113000-utc.log", TRACE),
    ])?;
    if sanitize_archive(&duplicate_file_alias).is_ok() {
        return Err("duplicate logical file with a second root alias was accepted".to_owned());
    }
    Ok(())
}

#[test]
fn named_volume_archive_projects_only_typed_fields() -> Result<(), String> {
    let bytes = match std::env::var_os("VELNOR_RUNNER_HOME_ARCHIVE_FIXTURE") {
        Some(path) => std::fs::read(path).map_err(|error| error.to_string())?,
        None => archive(&[
            (
                "_diag/Runner_20261021-113000-utc.log",
                b"synthetic runner diagnostic\n",
            ),
            (
                "_diag/Worker_20261021-113001-utc.log",
                b"synthetic worker diagnostic\n",
            ),
        ])?,
    };
    let projection = sanitize_archive(&bytes).map_err(|error| format!("{error:?}"))?;
    let projection = String::from_utf8(projection).map_err(|error| error.to_string())?;
    let expected = "{\"schema\":1,\"phase\":\"runner_trace\",\"complete\":true,\"runner_log_present\":true,\"worker_log_present\":true,\"events\":[],\"unrecognized_lines\":2,\"omitted_events\":0}";
    if projection != expected {
        return Err("named-volume archive did not produce its fixed sanitized summary".to_owned());
    }
    if projection.contains("synthetic runner diagnostic")
        || projection.contains("synthetic worker diagnostic")
    {
        return Err("raw named-volume log text reached the sanitized projection".to_owned());
    }
    Ok(())
}

#[test]
fn archive_accepts_two_official_size_traces_with_header_overhead_and_bounded_omissions()
-> Result<(), String> {
    const OFFICIAL_TRACE_LIMIT: usize = 8 * 1024 * 1024;
    const HEADER_AND_NEWLINE_OVERHEAD: usize = 64 * 1024;
    let trace_size = OFFICIAL_TRACE_LIMIT + HEADER_AND_NEWLINE_OVERHEAD;
    let runner = super::common::trace_at_size(trace_size);
    let worker = super::common::trace_at_size(trace_size);
    let bytes = archive(&[(RUNNER_NAME, &runner), (WORKER_NAME, &worker)])?;
    let projection = sanitize_archive(&bytes).map_err(|error| format!("{error:?}"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&projection).map_err(|error| error.to_string())?;
    if bytes.len() > super::super::MAX_ARCHIVE_BYTES {
        return Err("two official-size trace files exceeded the archive bound".to_owned());
    }
    if value["complete"] != false
        || value["runner_log_present"] != true
        || value["worker_log_present"] != true
        || value["events"].as_array().map_or(0, Vec::len) != 4096
        || value["omitted_events"].as_u64().unwrap_or_default() == 0
    {
        return Err("bounded event projection did not record its omissions".to_owned());
    }
    if projection
        .windows(b"canary-message-9e26".len())
        .any(|window| window == b"canary-message-9e26")
    {
        return Err("large trace message text reached the durable projection".to_owned());
    }
    Ok(())
}

#[test]
fn archive_requires_complete_zero_block_terminator_and_zero_padding() -> Result<(), String> {
    let complete = valid_archive()?;
    let trailing_zeros = complete.iter().rev().take_while(|byte| **byte == 0).count();
    if trailing_zeros < 1024 {
        return Err("test archive did not contain standard tar zero-block padding".to_owned());
    }
    let data_end = complete.len() - trailing_zeros;
    let missing_terminator = &complete[..data_end];
    if sanitize_archive(missing_terminator).is_ok() {
        return Err("archive without terminal zero blocks was accepted".to_owned());
    }
    let single_zero_block = &complete[..data_end + 512];
    if sanitize_archive(single_zero_block).is_ok() {
        return Err("archive with one terminal zero block was accepted".to_owned());
    }
    let mut hidden_after_terminator = complete.clone();
    let first_padding_end = data_end + 512;
    hidden_after_terminator[first_padding_end] = b'x';
    if sanitize_archive(&hidden_after_terminator).is_ok() {
        return Err("nonzero bytes after the tar terminator were accepted".to_owned());
    }
    Ok(())
}

#[test]
fn normal_paging_directories_and_files_are_bounded_and_discarded() -> Result<(), String> {
    let bytes = archive_with_paging_logs()?;
    let projection = sanitize_archive(&bytes).map_err(|error| format!("{error:?}"))?;
    let text = String::from_utf8(projection).map_err(|error| error.to_string())?;
    if text.contains("private job page canary") || text.contains("private results block canary") {
        return Err("Runner paging payload reached the durable projection".to_owned());
    }
    if !text.contains("\"runner_log_present\":true")
        || !text.contains("\"worker_log_present\":true")
    {
        return Err("paging entries changed the typed Runner/Worker summary".to_owned());
    }
    for invalid in [
        "_diag/pages/not-a-page.log",
        "_diag/blocks/not-a-block",
        "_diag/other/private.log",
    ] {
        let invalid = archive(&[(invalid, b"private payload")])?;
        if sanitize_archive(&invalid).is_ok() {
            return Err("unrecognized diagnostic entry was accepted".to_owned());
        }
    }
    Ok(())
}

#[test]
fn rotated_and_self_update_logs_are_recognized_without_retaining_text() -> Result<(), String> {
    let old_runner = "Runner_20261021-102959-utc.log";
    let update = "SelfUpdate-20261021-102959.log";
    let bytes = archive(&[
        (old_runner, b"private rotated runner payload"),
        (RUNNER_NAME, TRACE),
        (WORKER_NAME, TRACE),
        (update, b"private self-update payload"),
        (
            "GitCheck_20261021-102959-utc.log",
            b"private Git check payload",
        ),
        (
            "ActionsCheck_20261021-102959-utc.log",
            b"private Actions check payload",
        ),
        (
            "NodeJsCheck_20261021-102959-utc.log",
            b"private Node check payload",
        ),
        (
            "InternetCheck_20261021-102959-utc.log",
            b"private Internet check payload",
        ),
    ])?;
    let projection = sanitize_archive(&bytes).map_err(|error| format!("{error:?}"))?;
    let text = String::from_utf8(projection).map_err(|error| error.to_string())?;
    if text.contains("private rotated runner payload")
        || text.contains("private self-update payload")
        || text.contains("private Git check payload")
        || text.contains("private Actions check payload")
        || text.contains("private Node check payload")
        || text.contains("private Internet check payload")
    {
        return Err("auxiliary diagnostic text reached the durable projection".to_owned());
    }
    let malformed = archive(&[("SelfUpdate-20261340-256199.log", b"x")])?;
    if sanitize_archive(&malformed).is_ok() {
        return Err("malformed self-update filename was accepted".to_owned());
    }
    let malformed_check = archive(&[("GitCheck_20261340-256199-utc.log", b"x")])?;
    if sanitize_archive(&malformed_check).is_ok() {
        return Err("malformed Runner check filename was accepted".to_owned());
    }
    Ok(())
}

#[test]
fn archive_rejects_oversized_members_and_invalid_log_timestamps() -> Result<(), String> {
    let oversized = vec![b'x'; MAX_LOG_FILE_BYTES + 1];
    let large = archive(&[(RUNNER_NAME, &oversized), (WORKER_NAME, TRACE)])?;
    if sanitize_archive(&large).is_ok() {
        return Err("oversized log member was accepted".to_owned());
    }
    for timestamp in [
        b"2026-02-29 11:30:00Z".as_slice(),
        b"2026-10-21 24:00:00Z".as_slice(),
        b"2026-10-21 11:60:00Z".as_slice(),
        b"+026-10-21 11:30:00Z".as_slice(),
        b"2026-10-21 +1:30:00Z".as_slice(),
        b"0000-01-01 00:00:00Z".as_slice(),
    ] {
        if valid_timestamp(timestamp) {
            return Err(format!("invalid timestamp accepted: {timestamp:?}"));
        }
    }
    if !valid_timestamp(b"2024-02-29 23:59:59Z") {
        return Err("valid leap-day timestamp was rejected".to_owned());
    }
    Ok(())
}

#[test]
fn archive_rejects_pax_extension_entries() -> Result<(), String> {
    let bytes = pax_archive()?;
    if sanitize_archive(&bytes) != Err(HostError::Ownership) {
        return Err("PAX extension metadata was accepted".to_owned());
    }
    Ok(())
}

#[test]
fn archive_rejects_more_than_the_entry_limit() -> Result<(), String> {
    let bytes = too_many_entries_archive()?;
    if sanitize_archive(&bytes) != Err(HostError::Ownership) {
        return Err("archive over the entry limit was accepted".to_owned());
    }
    Ok(())
}
