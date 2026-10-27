// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub(crate) fn install_shasum_double(bin: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let real_shasum = resolve_shasum()?;
    let wrapper = bin.join("shasum");
    fs::write(&wrapper, SHASUM_DOUBLE)?;
    let mut permissions = fs::metadata(&wrapper)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&wrapper, permissions)?;
    Ok(real_shasum)
}

fn resolve_shasum() -> Result<PathBuf, Box<dyn Error>> {
    let original_path = env::var_os("PATH").ok_or("PATH is unavailable")?;
    for directory in env::split_paths(&original_path) {
        let candidate = directory.join("shasum");
        if candidate.is_file() && fs::metadata(&candidate)?.permissions().mode() & 0o111 != 0 {
            return Ok(fs::canonicalize(candidate)?);
        }
    }
    Err("real shasum is unavailable on the original PATH".into())
}

pub(crate) fn macho_bytes(suffix: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 1];
    bytes.resize(32, 0);
    bytes.extend_from_slice(suffix);
    bytes
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

const SHASUM_DOUBLE: &str = r#"#!/bin/sh
set -eu
failure="${SHASUM_FAILURE_AT:-none}"
selected=""
case "$failure:$#" in
  initial-helper:3)
    if [ "$1" = "-a" ] && [ "$2" = "256" ] && [ "$3" = "crates/velnor-runner/target/release/velnor-runner-attestation-helper" ]; then selected=initial-helper; fi
    ;;
  copied-helper:3)
    if [ "$1" = "-a" ] && [ "$2" = "256" ] && [ "$3" = "velnor-runner-attestation-helper" ]; then selected=copied-helper; fi
    ;;
esac
if [ -z "$selected" ]; then
  exec "$REAL_SHASUM" "$@"
fi
output_file="${SHASUM_LOG}.output"
if "$REAL_SHASUM" "$@" >"$output_file"; then
  real_status=0
else
  real_status=$?
fi
cat "$output_file"
output="$(cat "$output_file")"
if [ "$real_status" -ne 0 ]; then
  printf 'stage=%s real_status=%s emitted=%s forced_status=not-applied\n' "$selected" "$real_status" "$output" >> "$SHASUM_LOG"
  exit "$real_status"
fi
printf 'stage=%s real_status=0 emitted=%s forced_status=43\n' "$selected" "$output" >> "$SHASUM_LOG"
exit 43
"#;
