pub(super) const STREAM_FILE_WRITE: &str = "output.write_all(bytes)";

/// Tool-file names allowed only in `toolfiles.rs` (read-only routing and
/// inspection): owned Mise files plus foreign Rust files for routing.
pub(super) const TOOLFILES_ONLY: &[&str] = &[
    "mise.toml",
    "mise.lock",
    "rust-toolchain.toml",
    "Cargo.toml",
    "Cargo.lock",
];

pub(super) fn check_source_sites(name: &str, source: &str, violations: &mut Vec<String>) {
    let compact = source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" .", ".");
    let is_filter_process = name == "bounded_file_filter_process.rs";
    let is_filter = name == "bounded_file_filter.rs";
    let is_command_stream = name == "command_stream.rs";
    let is_command_progress = name == "command_progress.rs";
    let write_expression = "output.write_all(&buffer[..count])";
    let diagnostic_expression = ".write_all(bytes)";
    let metadata_expression = "std::fs::symlink_metadata(program)";
    super::path_policy::check_path_openers(name, source, violations);
    if name == "requests_executable_lookup.rs"
        && !super::rustup_policy::has_pinned_mise_rustup_payload(source)
    {
        violations.push(format!(
            "{name}: Rustup lookup must use one fixed payload through Mise."
        ));
    }
    if is_filter_process && compact.matches(write_expression).count() != 1 {
        violations.push(format!(
            "{name}: expected exactly one bounded output write `{write_expression}`"
        ));
    }
    if is_filter && compact.matches(metadata_expression).count() != 1 {
        violations.push(format!(
            "{name}: expected exactly one read-only program metadata check `{metadata_expression}`"
        ));
    }
    if is_command_stream && compact.matches(STREAM_FILE_WRITE).count() != 1 {
        violations.push(format!(
            "{name}: expected exactly one caller-owned bounded stream write `{STREAM_FILE_WRITE}`"
        ));
    }
    if !is_command_stream && !is_command_progress && compact.contains(STREAM_FILE_WRITE) {
        violations.push(format!(
            "{name}: caller-owned stream write is outside its approved module"
        ));
    }
    if is_command_progress && compact.matches(diagnostic_expression).count() != 1 {
        violations.push(format!(
            "{name}: expected exactly one borrowed-diagnostic write `{diagnostic_expression}`"
        ));
    }
    if !is_command_progress && !is_command_stream && compact.contains(diagnostic_expression) {
        violations.push(format!(
            "{name}: borrowed-diagnostic write is outside its approved module"
        ));
    }
    if !is_filter_process && compact.contains(write_expression) {
        violations.push(format!(
            "{name}: bounded output write is outside its approved module"
        ));
    }
    if !is_filter && compact.contains(metadata_expression) {
        violations.push(format!(
            "{name}: program metadata check is outside its approved module"
        ));
    }
}
