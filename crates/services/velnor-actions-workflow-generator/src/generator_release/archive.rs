//! Strict sidecar parsing and safe extraction for release candidate archives.

const SIDECAR_AWK: &str = "NR == 1 { if (NF != 2 || length($1) != 64 || $1 !~ /^[0-9a-f]+$/ || $2 != expected) exit 1; print $1; next } { exit 1 } END { if (NR != 1) exit 1 }";

#[cfg(test)]
const EXTRACTOR: &str =
    include_str!("../../../../../scripts/generator-release/extract-candidate.py");

/// Emit one AWK command that accepts only the candidate's exact checksum row.
pub(super) fn sidecar_digest_command(sidecar: &str, binary: &str) -> String {
    format!("awk -v expected='{binary}' '{SIDECAR_AWK}' '{sidecar}'")
}

/// Validate the complete archive before extracting any candidate files.
pub(super) fn extraction_script(
    directory: &str,
    archive: &str,
    binary: &str,
    sidecar: &str,
    provenance: &str,
) -> String {
    format!(
        "set -eu\nbash scripts/with-owned-archive-guard.sh -- python3 scripts/generator-release/extract-candidate.py '{directory}/{archive}' '{directory}' '{binary}' '{sidecar}' '{provenance}'"
    )
}
#[cfg(test)]
mod tests;
