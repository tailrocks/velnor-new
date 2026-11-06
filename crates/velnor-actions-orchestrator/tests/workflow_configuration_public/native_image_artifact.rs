// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path, process::Command};

fn make_archive(path: &Path, case: &str) -> Result<(), Box<dyn Error>> {
    let script = r"import sys, zipfile
path, case = sys.argv[1:]
with zipfile.ZipFile(path, 'w') as z:
    z.writestr('candidate.oci.tar', b'exact candidate')
    if case not in ('symlink', 'traversal', 'missing'): z.writestr('candidate.json', b'exact record')
    if case == 'duplicate': z.writestr('candidate.json', b'other')
    if case == 'extra': z.writestr('extra', b'other')
    if case == 'oversize':
        z.writestr('third', b'x')
        z.writestr('fourth', b'x')
    if case == 'traversal': z.writestr('../escape', b'other')
    if case == 'symlink':
        info = zipfile.ZipInfo('candidate.json')
        info.create_system = 3
        info.external_attr = (0o120777 << 16)
        z.writestr(info, 'target')
if case == 'truncated':
    with open(path, 'rb+') as f:
        f.truncate(20)
if case == 'oversize':
    import struct
    with open(path, 'rb+') as f:
        data = bytearray(f.read())
        position = 0
        for _ in range(4):
            position = data.index(b'PK\x01\x02', position)
            struct.pack_into('<I', data, position + 24, 0xfffffffe)
            position += 46
        f.seek(0)
        f.write(data)
";
    let output = Command::new("python3")
        .args(["-c", script])
        .arg(path)
        .arg(case)
        .output()?;
    if !output.status.success() {
        return Err("fixture archive creation failed".into());
    }
    Ok(())
}

#[test]
fn native_artifact_extraction_requires_a_closed_regular_inventory() -> Result<(), Box<dyn Error>> {
    let fixture = tempfile::tempdir()?;
    let extractor = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../velnor-actions-workflow-renderer/src/schema2_ci_image_artifact.py");
    for case in [
        "valid",
        "duplicate",
        "missing",
        "extra",
        "traversal",
        "symlink",
        "truncated",
        "oversize",
    ] {
        let archive = fixture.path().join(format!("{case}.zip"));
        let destination = fixture.path().join(case);
        fs::create_dir(&destination)?;
        make_archive(&archive, case)?;
        let second = if case == "traversal" {
            "../escape"
        } else {
            "candidate.json"
        };
        let mut command = Command::new("python3");
        command.arg(&extractor).arg(&archive).arg(&destination);
        command.args(["candidate.oci.tar", second]);
        if case == "oversize" {
            command.args(["third", "fourth"]);
        }
        let result = command.output()?;
        assert_eq!(
            result.status.success(),
            case == "valid",
            "{case}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        if case == "oversize" {
            assert!(String::from_utf8_lossy(&result.stderr).contains("artifact too large"));
        }
        if case == "valid" {
            assert_eq!(
                fs::read(destination.join("candidate.oci.tar"))?,
                b"exact candidate"
            );
            assert_eq!(
                fs::read(destination.join("candidate.json"))?,
                b"exact record"
            );
        }
        assert!(!fixture.path().join("escape").exists());
    }
    Ok(())
}
