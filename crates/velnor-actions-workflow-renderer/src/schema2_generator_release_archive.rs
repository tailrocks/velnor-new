//! Strict sidecar parsing and native extraction for release candidate archives.

const SIDECAR_AWK: &str = "NR == 1 { if (NF != 2 || length($1) != 64 || $1 !~ /^[0-9a-f]+$/ || $2 != expected) exit 1; print $1; next } { exit 1 } END { if (NR != 1) exit 1 }";

/// Emit one AWK command that accepts only the candidate's exact checksum row.
pub(super) fn sidecar_digest_command(sidecar: &str, binary: &str) -> String {
    format!("awk -v expected='{binary}' '{SIDECAR_AWK}' '{sidecar}'")
}

/// Build the trusted source guard before extracting any candidate files.
pub(super) fn extraction_script(
    directory: &str,
    archive: &str,
    binary: &str,
    sidecar: &str,
    provenance: &str,
) -> String {
    format!(
        "set -eu\nbash scripts/with-owned-archive-guard.sh -- .velnor/archive-guard/bin/velnor-archive-guard extract-candidate '{directory}/{archive}' '{directory}' '{binary}' '{sidecar}' '{provenance}'"
    )
}

#[cfg(test)]
mod tests {
    use super::{SIDECAR_AWK, extraction_script, sidecar_digest_command};
    use std::error::Error;
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[test]
    fn extraction_uses_source_guard_before_candidate_execution() {
        let script = extraction_script("assets", "candidate.tar", "cli", "cli.sha256", "cli.json");
        assert_eq!(
            script,
            "set -eu\nbash scripts/with-owned-archive-guard.sh -- .velnor/archive-guard/bin/velnor-archive-guard extract-candidate 'assets/candidate.tar' 'assets' 'cli' 'cli.sha256' 'cli.json'"
        );
        assert!(!script.contains("python"));
        assert!(!script.contains("ruby"));
    }

    #[test]
    fn checksum_sidecar_must_name_the_exact_binary() -> Result<(), Box<dyn Error>> {
        for (row, accepted) in [
            (format!("{}  cli\n", "a".repeat(64)), true),
            (format!("{}  another-binary\n", "a".repeat(64)), false),
            (format!("{}  cli\n", "A".repeat(64)), false),
            (format!("{}  cli\nsecond-row\n", "a".repeat(64)), false),
        ] {
            let mut child = Command::new("awk")
                .args(["-v", "expected=cli", SIDECAR_AWK])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()?;
            child
                .stdin
                .take()
                .ok_or("missing AWK stdin")?
                .write_all(row.as_bytes())?;
            assert_eq!(child.wait()?.success(), accepted, "row: {row:?}");
        }
        assert!(sidecar_digest_command("cli.sha256", "cli").contains("expected='cli'"));
        Ok(())
    }
}
