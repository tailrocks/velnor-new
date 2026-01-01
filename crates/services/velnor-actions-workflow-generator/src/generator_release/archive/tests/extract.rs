use super::{BINARY, CHECKSUM, PROVENANCE};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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

fn run_extractor(directory: &Path, archive: &Path) -> Result<bool, Box<dyn Error>> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts");
    let status = Command::new("python3")
        .arg(repository.join("generator-release/extract-candidate.py"))
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
