// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use sha1_checked::{Digest, Sha1};
use std::{
    error::Error,
    io::{self, Read, Write},
};

const TREE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TARGET: &str = ".velnor/runner.lock";

pub(super) fn tree_api(path: &str, state: &Value) -> Result<Value, Box<dyn Error>> {
    if let Some(source) = path.strip_prefix("repos/tailrocks/velnor-new/git/commits/") {
        if state["source_commit"] != source {
            return Err("unknown fixture source".into());
        }
        return Ok(json!({"sha":source,"tree":{"sha":TREE}}));
    }
    if path != format!("repos/tailrocks/velnor-new/git/trees/{TREE}") {
        return Err("unknown fixture tree".into());
    }
    let fault = state["snapshot_fault"].as_str().unwrap_or("");
    let files = state["source_files"]
        .as_object()
        .ok_or("source files missing")?;
    let mut entries = Vec::new();
    for (path, encoded) in files {
        if path == TARGET && fault == "tree_missing" {
            continue;
        }
        let bytes = STANDARD.decode(encoded.as_str().ok_or("bad source encoding")?)?;
        let mode = if path == TARGET && fault == "tree_nonregular" {
            "120000"
        } else {
            "100644"
        };
        let oid = if path == TARGET && fault == "tree_blob_mismatch" {
            "c".repeat(40)
        } else {
            blob_sha(&bytes)?
        };
        let entry = json!({"path":path,"mode":mode,"type":"blob","sha":oid});
        entries.push(entry.clone());
        if path == TARGET && fault == "tree_duplicate" {
            entries.push(entry);
        }
    }
    Ok(json!({"sha":TREE,"truncated":fault == "tree_truncated","tree":entries}))
}

pub(super) fn write_tar(route: &str, state: &Value) -> Result<(), Box<dyn Error>> {
    let route = route.split('?').next().ok_or("bad archive route")?;
    let source = route
        .strip_prefix("repos/tailrocks/velnor-new/tarball/")
        .ok_or("bad archive route")?;
    if state["source_commit"] != source {
        return Err("unknown fixture source".into());
    }
    let fault = state["snapshot_fault"].as_str().unwrap_or("");
    let files = state["source_files"]
        .as_object()
        .ok_or("source files missing")?;
    let stdout = io::stdout();
    if fault == "compressed_cap" {
        io::copy(
            &mut io::repeat(0).take(64 * 1024 * 1024 + 1),
            &mut stdout.lock(),
        )?;
        return Ok(());
    }
    let encoder = GzEncoder::new(stdout.lock(), Compression::fast());
    let mut tar = tar::Builder::new(encoder);
    write_headers(&mut tar, source, fault)?;
    for (path, encoded) in files {
        if path == TARGET && fault == "archive_missing" {
            continue;
        }
        let mut bytes = STANDARD.decode(encoded.as_str().ok_or("bad source encoding")?)?;
        if path == TARGET && fault == "archive_altered" {
            bytes.push(1);
        }
        if path == TARGET && fault == "file_cap" {
            bytes.resize(16 * 1024 * 1024 + 1, 0);
        }
        let mut header = tar::Header::new_gnu();
        header.set_mode(if path == TARGET && fault == "archive_mode" {
            0o755
        } else {
            0o644
        });
        if path == TARGET && fault == "archive_symlink" {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_link_name("target")?;
            header.set_cksum();
            tar.append_data(&mut header, format!("fixture/{path}"), io::empty())?;
        } else {
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, format!("fixture/{path}"), bytes.as_slice())?;
            if path == TARGET && fault == "archive_duplicate" {
                tar.append_data(&mut header, format!("fixture/{path}"), bytes.as_slice())?;
            }
        }
    }
    let encoder = tar.into_inner()?;
    encoder.finish()?.flush()?;
    Ok(())
}

fn write_headers<W: Write>(
    tar: &mut tar::Builder<W>,
    source: &str,
    fault: &str,
) -> Result<(), Box<dyn Error>> {
    let pax = format!("52 comment={source}\n");
    let mut global = tar::Header::new_gnu();
    global.set_entry_type(tar::EntryType::XGlobalHeader);
    global.set_mode(0o666);
    global.set_size(pax.len() as u64);
    global.set_cksum();
    tar.append_data(&mut global, "pax_global_header", pax.as_bytes())?;
    if fault == "other_prefix" || fault == "traversal" {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(0);
        let name = if fault == "traversal" {
            "fixture/../evil"
        } else {
            "other/evil"
        };
        header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
        header.set_cksum();
        tar.append(&header, io::empty())?;
    }
    if fault == "expanded_cap" {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(256 * 1024 * 1024 + 1024);
        header.set_cksum();
        tar.append_data(
            &mut header,
            "fixture/unselected-padding",
            io::repeat(0).take(256 * 1024 * 1024 + 1024),
        )?;
    }
    Ok(())
}

fn blob_sha(bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hash = Sha1::new();
    hash.update(format!("blob {}\0", bytes.len()).as_bytes());
    hash.update(bytes);
    let result = hash.try_finalize();
    if result.has_collision() {
        return Err("fixture collision".into());
    }
    let mut output = String::with_capacity(40);
    for byte in result.hash() {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    Ok(output)
}
