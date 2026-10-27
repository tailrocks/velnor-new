// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

const RECORD_SOURCE: &str = include_str!("../src/job_image_authority_record.py");
const OCI_SOURCE: &str = include_str!("../src/job_image_authority_oci.py");
const TAR_SOURCE: &str = include_str!("../src/job_image_authority_tar.py");

const RECORD_FILE: &str = "job_image_authority_record.py";
const OCI_FILE: &str = "job_image_authority_oci.py";
const TAR_FILE: &str = "job_image_authority_tar.py";

const DRIVER: &str = r#"import sys
from pathlib import Path

ROOT = Path(sys.argv[1])

sys.path.insert(0, str(ROOT))

import job_image_authority_oci as oci_module
import job_image_authority_record as record_module
import job_image_authority_tar as tar_module


def run_record_ok(path):
    record_module.validate_record(record_module.load_record(Path(path)))
    print("RECORD-OK")


def run_record_bad(path):
    try:
        record_module.validate_record(record_module.load_record(Path(path)))
    except record_module.AuthorityError as error:
        print(f"RECORD-REJECTED {error}")
        return
    raise SystemExit(3)


def run_archive_ok(path):
    result = oci_module.verify_oci_archive(Path(path))
    print("ARCHIVE-OK")
    print(f"archive_index_digest={result['archive_index_digest']}")
    print(f"archive_index_size_bytes={result['archive_index_size_bytes']}")
    print(f"platform_manifest_digest={result['platform_manifest_digest']}")
    print(f"platform_manifest_size_bytes={result['platform_manifest_size_bytes']}")
    print(f"image_configuration_digest={result['image_configuration_digest']}")
    print(f"image_configuration_size_bytes={result['image_configuration_size_bytes']}")
    print(f"layer_digests={','.join(result['layer_digests'])}")


def run_archive_bad(path):
    try:
        oci_module.verify_oci_archive(Path(path))
    except oci_module.AuthorityError as error:
        print(f"ARCHIVE-REJECTED {error}")
        return
    raise SystemExit(3)


def run_tar_member(name):
    try:
        first = tar_module._safe_member(name)
    except tar_module.AuthorityError as error:
        try:
            oci_module._safe_member(name)
        except oci_module.AuthorityError as other:
            if str(other) != str(error):
                raise SystemExit(4)
            print(f"TAR-MEMBER-REJECTED {error}")
            return
        raise SystemExit(4)
    try:
        second = oci_module._safe_member(name)
    except oci_module.AuthorityError:
        raise SystemExit(4)
    if second != first:
        raise SystemExit(4)
    print(f"TAR-MEMBER-OK {first}")


def run_tar_layer(name):
    try:
        first = tar_module._layer_path(name)
    except tar_module.AuthorityError as error:
        try:
            oci_module._layer_path(name)
        except oci_module.AuthorityError as other:
            if str(other) != str(error):
                raise SystemExit(4)
            print(f"TAR-LAYER-REJECTED {error}")
            return
        raise SystemExit(4)
    try:
        second = oci_module._layer_path(name)
    except oci_module.AuthorityError:
        raise SystemExit(4)
    if second != first:
        raise SystemExit(4)
    print(f"TAR-LAYER-OK {first}")


def main():
    mode = sys.argv[2]
    if mode == "record_ok":
        run_record_ok(sys.argv[3])
    elif mode == "record_bad":
        run_record_bad(sys.argv[3])
    elif mode == "archive_ok":
        run_archive_ok(sys.argv[3])
    elif mode == "archive_bad":
        run_archive_bad(sys.argv[3])
    elif mode == "tar_member":
        run_tar_member(sys.argv[3])
    elif mode == "tar_layer":
        run_tar_layer(sys.argv[3])
    else:
        raise SystemExit(2)


main()
"#;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn forbid(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Err(fail(&format!("{label} kept:{needle}")))
    } else {
        Ok(())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

fn module_imports(source: &str) -> Vec<String> {
    let mut imports: Vec<String> = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix("import ")
                .or_else(|| {
                    line.strip_prefix("from ")
                        .and_then(|rest| rest.split(' ').next())
                })
                .map(str::to_owned)
        })
        .collect();
    imports.sort();
    imports.dedup();
    imports
}

fn check_fixed_source(
    name: &str,
    source: &str,
    expected_imports: &[&str],
) -> Result<(), Box<dyn Error>> {
    for forbidden in [
        "import candidate",
        "from candidate",
        "candidate-built",
        "candidate-supplied",
        "subprocess",
        "os.system",
        "os.exec",
        "os.popen",
        "os.spawn",
        "os.environ",
        "os.getenv",
        "shutil",
        "tempfile",
        "argparse",
        "tomllib",
        "socket",
        "urllib",
        "http.client",
        "check_output",
        "Popen",
        "eval(",
        "exec(",
        "compile(",
        "__import__",
        "GITHUB_",
        "read_text",
    ] {
        forbid(source, forbidden, &format!("{name} fixed input"))?;
    }
    let imports = module_imports(source);
    let expected: Vec<String> = expected_imports
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
    if imports != expected {
        return Err(fail(&format!(
            "{name} left its standard-library-only imports"
        )));
    }
    Ok(())
}

fn check_sources_are_fixed() -> Result<(), Box<dyn Error>> {
    check_fixed_source(RECORD_FILE, RECORD_SOURCE, &["json", "re"])?;
    check_fixed_source(TAR_FILE, TAR_SOURCE, &["hashlib", "pathlib", "zlib"])?;
    check_fixed_source(
        OCI_FILE,
        OCI_SOURCE,
        &[
            "gzip", "hashlib", "json", "pathlib", "re", "tarfile", "zlib",
        ],
    )?;
    for (name, source) in [
        (RECORD_FILE, RECORD_SOURCE),
        (OCI_FILE, OCI_SOURCE),
        (TAR_FILE, TAR_SOURCE),
    ] {
        for marker in ["AuthorityError", "raise AuthorityError"] {
            require(source, marker, &format!("{name} fixed input"))?;
        }
    }
    require(RECORD_SOURCE, "validate_record", "record fixed input")?;
    require(RECORD_SOURCE, "load_record", "record fixed input")?;
    require(OCI_SOURCE, "verify_oci_archive", "oci fixed input")?;
    require(TAR_SOURCE, "_safe_member", "tar fixed input")?;
    require(TAR_SOURCE, "_scan_archive_extensions", "tar fixed input")?;
    Ok(())
}

#[path = "gc1_t45_accept.rs"]
mod gc1_t45_accept;
#[path = "gc1_t45_archive.rs"]
mod gc1_t45_archive;
#[path = "gc1_t45_inputs.rs"]
mod gc1_t45_inputs;
#[path = "gc1_t45_reject_a.rs"]
mod gc1_t45_reject_a;
#[path = "gc1_t45_reject_b.rs"]
mod gc1_t45_reject_b;
pub(crate) use gc1_t45_accept::*;
pub(crate) use gc1_t45_archive::*;
pub(crate) use gc1_t45_inputs::*;
pub(crate) use gc1_t45_reject_a::*;
