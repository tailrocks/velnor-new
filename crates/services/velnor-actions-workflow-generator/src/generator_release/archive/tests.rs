use super::{EXTRACTOR, SIDECAR_AWK, extraction_script, sidecar_digest_command};
use std::error::Error;
use std::io::Write;
use std::process::{Command, Stdio};

const BINARY: &str = concat!(
    "velnor-actions-",
    env!("CARGO_PKG_VERSION"),
    "-x86_64-unknown-linux-gnu"
);
const CHECKSUM: &str = concat!(
    "velnor-actions-",
    env!("CARGO_PKG_VERSION"),
    "-x86_64-unknown-linux-gnu.sha256"
);
const PROVENANCE: &str = concat!(
    "velnor-actions-",
    env!("CARGO_PKG_VERSION"),
    "-x86_64-unknown-linux-gnu.provenance.json"
);

mod extract;

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
    assert!(command.contains("expected='velnor-actions-0.1.7"));
    Ok(())
}
