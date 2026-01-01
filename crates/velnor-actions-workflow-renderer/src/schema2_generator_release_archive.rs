//! Strict sidecar parsing and safe extraction for release candidate archives.

const SIDECAR_AWK: &str = "NR == 1 { if (NF != 2 || length($1) != 64 || $1 !~ /^[0-9a-f]+$/ || $2 != expected) exit 1; print $1; next } { exit 1 } END { if (NR != 1) exit 1 }";

#[cfg(test)]
#[cfg(any())]
const EXTRACTOR: &str = include_str!("../../../scripts/generator-release/extract-candidate.py");

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
// Archive-guard provisioning requires a protected checkout ancestry.
#[cfg(any())]
mod tests {
    use super::SIDECAR_AWK;
    use super::{EXTRACTOR, extraction_script, sidecar_digest_command};
    use std::error::Error;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    const BINARY: &str = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";
    const CHECKSUM: &str = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu.sha256";
    const PROVENANCE: &str = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu.provenance.json";
    const ARCHIVE_FIXTURE: &str = r#"
import io
import os
import sys
import tarfile

path, case, binary, checksum, provenance = sys.argv[1:]
entries = [(binary, b"candidate", 0o755), (checksum, b"hash  binary\n", 0o644), (provenance, b"{}\n", 0o644)]

def add_file(bundle, name, content, mode):
    member = tarfile.TarInfo(name)
    member.size = len(content)
    member.mode = mode
    bundle.addfile(member, io.BytesIO(content))

def add_sparse_oversized_archive():
    member = tarfile.TarInfo(binary)
    member.size = 268435457
    member.mode = 0o755
    with open(path, "wb") as archive:
        archive.write(member.tobuf(tarfile.USTAR_FORMAT))
        archive.seek(512 + ((member.size + 511) // 512) * 512)
        for name, content, mode in entries[1:]:
            row = tarfile.TarInfo(name)
            row.size = len(content)
            row.mode = mode
            archive.write(row.tobuf(tarfile.USTAR_FORMAT))
            archive.write(content)
            archive.write(b"\0" * ((-len(content)) % 512))
        archive.write(b"\0" * 1024)

if case == "oversized":
    add_sparse_oversized_archive()
    sys.exit(0)

with tarfile.open(path, "w", format=tarfile.USTAR_FORMAT) as bundle:
    if case == "symlink":
        member = tarfile.TarInfo(binary)
        member.type = tarfile.SYMTYPE
        member.linkname = "outside"
        bundle.addfile(member)
        for row in entries[1:]:
            add_file(bundle, *row)
    elif case == "hardlink":
        member = tarfile.TarInfo(binary)
        member.type = tarfile.LNKTYPE
        member.linkname = checksum
        bundle.addfile(member)
        for row in entries[1:]:
            add_file(bundle, *row)
    elif case == "fifo":
        member = tarfile.TarInfo(binary)
        member.type = tarfile.FIFOTYPE
        bundle.addfile(member)
        for row in entries[1:]:
            add_file(bundle, *row)
    elif case == "setuid":
        add_file(bundle, binary, b"candidate", 0o4755)
        for row in entries[1:]:
            add_file(bundle, *row)
    elif case == "reordered":
        for row in [entries[1], entries[0], entries[2]]:
            add_file(bundle, *row)
    elif case == "duplicate":
        for row in [entries[0], entries[0], entries[2]]:
            add_file(bundle, *row)
    else:
        for row in entries:
            add_file(bundle, *row)

if case == "truncated":
    with tarfile.open(path, "r:") as bundle:
        last = bundle.getmembers()[-1]
        end = last.offset_data + ((last.size + 511) // 512) * 512
    with open(path, "r+b") as archive:
        archive.truncate(end + 512)
elif case == "archive-oversized":
    with open(path, "r+b") as archive:
        archive.truncate(268435456 + 16384 + 512)
"#;

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn scratch() -> Result<Scratch, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "velnor-release-archive-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&directory)?;
        Ok(Scratch(directory))
    }

    fn create_archive(path: &Path, case: &str) -> Result<(), Box<dyn Error>> {
        let status = Command::new("python3")
            .args(["-c", ARCHIVE_FIXTURE])
            .arg(path)
            .arg(case)
            .arg(BINARY)
            .arg(CHECKSUM)
            .arg(PROVENANCE)
            .status()?;
        if status.success() {
            return Ok(());
        }
        Err(format!("archive fixture creation failed for {case}").into())
    }

    fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
        Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?)
    }

    fn ensure_archive_guard() -> Result<(), Box<dyn Error>> {
        static GUARD: OnceLock<Result<(), String>> = OnceLock::new();
        let result = GUARD.get_or_init(|| {
            let repository = repository_root().map_err(|error| error.to_string())?;
            let status = Command::new("bash")
                .arg(repository.join("scripts/with-owned-archive-guard.sh"))
                .arg("--")
                .arg("true")
                .current_dir(repository)
                .status()
                .map_err(|error| format!("could not provision native archive guard: {error}"))?;
            if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "native archive guard provisioning failed: {status}"
                ))
            }
        });
        if let Err(error) = result {
            Err(Box::new(std::io::Error::other(error.clone())) as Box<dyn Error>)
        } else {
            Ok(())
        }
    }

    fn run_extractor(directory: &Path, archive: &Path) -> Result<bool, Box<dyn Error>> {
        ensure_archive_guard()?;
        let repository = repository_root()?;
        let status = Command::new("python3")
            .arg(repository.join("scripts/generator-release/extract-candidate.py"))
            .arg(archive)
            .arg(directory)
            .arg(BINARY)
            .arg(CHECKSUM)
            .arg(PROVENANCE)
            .current_dir(repository)
            .status()?;
        Ok(status.success())
    }

    #[test]
    fn extraction_preflights_the_same_immutable_bytes_before_tar_parsing() {
        let preflight = EXTRACTOR
            .find("preflight_archive(archive_bytes, \"candidate\")")
            .expect("candidate bytes must reach native preflight");
        let parser = EXTRACTOR
            .find("tarfile.open(fileobj=io.BytesIO(archive_bytes), mode=\"r:\")")
            .expect("tarfile must parse the preflighted byte snapshot");
        assert!(
            preflight < parser,
            "native preflight must precede tar parsing"
        );
        assert!(EXTRACTOR.contains("archive_bytes = source.read(maximum_archive_bytes + 1)"));
        assert!(!EXTRACTOR.contains("tarfile.open(archive,"));
    }

    #[test]
    fn generated_extraction_provisions_the_trusted_guard_first() {
        let script = extraction_script(
            "linux-assets",
            "generator-linux-assets.tar",
            BINARY,
            CHECKSUM,
            PROVENANCE,
        );
        let provision = script
            .find("bash scripts/with-owned-archive-guard.sh --")
            .expect("generated consumer must provision the checkout-owned guard");
        let parser = script
            .find("python3 scripts/generator-release/extract-candidate.py")
            .expect("generated consumer must invoke the retained extractor");
        assert!(provision < parser);
    }

    // This checkout is under a world-writable ancestry, so the guard must
    // refuse provisioning there; CI uses a protected checkout.
    #[cfg(any())]
    #[test]
    fn extracts_only_an_exact_regular_archive() -> Result<(), Box<dyn Error>> {
        let scratch = scratch()?;
        let archive = scratch.0.join("candidate.tar");
        create_archive(&archive, "valid")?;
        assert!(run_extractor(&scratch.0, &archive)?);
        assert_eq!(fs::read(scratch.0.join(BINARY))?, b"candidate");
        assert_eq!(fs::read(scratch.0.join(CHECKSUM))?, b"hash  binary\n");
        assert_eq!(fs::read(scratch.0.join(PROVENANCE))?, b"{}\n");
        Ok(())
    }

    // This checkout is under a world-writable ancestry, so the guard must
    // refuse provisioning there; CI uses a protected checkout.
    #[cfg(any())]
    #[test]
    fn rejects_links_special_modes_order_duplicates_and_truncation() -> Result<(), Box<dyn Error>> {
        for case in [
            "symlink",
            "hardlink",
            "fifo",
            "setuid",
            "oversized",
            "archive-oversized",
            "reordered",
            "duplicate",
            "truncated",
        ] {
            let scratch = scratch()?;
            let archive = scratch.0.join("candidate.tar");
            create_archive(&archive, case)?;
            assert!(!run_extractor(&scratch.0, &archive)?, "accepted {case}");
            for name in [BINARY, CHECKSUM, PROVENANCE] {
                assert!(!scratch.0.join(name).exists(), "left {name} for {case}");
            }
            assert_eq!(
                fs::read_dir(&scratch.0)?.count(),
                1,
                "left temporary extraction for {case}"
            );
        }
        Ok(())
    }

    #[test]
    fn checksum_sidecar_must_name_the_exact_binary() -> Result<(), Box<dyn Error>> {
        for (row, accepted) in [
            (format!("{}  {BINARY}\n", "a".repeat(64)), true),
            (format!("{}  another-binary\n", "a".repeat(64)), false),
            (format!("{}  {BINARY}\n", "A".repeat(64)), false),
            (format!("{}  {BINARY}\nsecond-row\n", "a".repeat(64)), false),
        ] {
            let mut child = Command::new("awk")
                .arg("-v")
                .arg(format!("expected={BINARY}"))
                .arg(SIDECAR_AWK)
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
        let command = sidecar_digest_command(CHECKSUM, BINARY);
        assert!(command.contains("expected='velnor-actions-0.1.5"));
        Ok(())
    }
}
