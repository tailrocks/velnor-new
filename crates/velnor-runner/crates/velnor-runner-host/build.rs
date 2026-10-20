//! Bind validated release source and helper identities into the host crate.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

const SOURCE_ENV: &str = "VELNOR_SOURCE_SHA";
const HELPER_ENV: &str = "VELNOR_ATTESTATION_HELPER_SHA256";
const COMPILED_SOURCE_ENV: &str = "VELNOR_COMPILED_SOURCE_SHA";
const COMPILED_HELPER_ENV: &str = "VELNOR_COMPILED_ATTESTATION_HELPER_SHA256";

fn main() {
    println!("cargo:rerun-if-env-changed={SOURCE_ENV}");
    println!("cargo:rerun-if-env-changed={HELPER_ENV}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/compile_identity.rs");
    if let Err(error) = configure() {
        eprintln!("runner release identity: {error}");
        process::exit(1);
    }
}

fn configure() -> Result<(), String> {
    let source = read_input(SOURCE_ENV)?;
    let helper = read_input(HELPER_ENV)?;
    match (source, helper) {
        (None, None) => write_unavailable(),
        (Some(source), Some(helper)) => write_available(&source, &helper),
        _ => Err("source and helper digests must be supplied together".to_owned()),
    }
}

fn read_input(name: &str) -> Result<Option<String>, String> {
    env::var_os(name).map_or(Ok(None), |value| {
        value
            .into_string()
            .map(Some)
            .map_err(|_| format!("{name} must be UTF-8 lowercase hexadecimal"))
    })
}

fn write_available(source: &str, helper: &str) -> Result<(), String> {
    let source_bytes = parse_hex::<20>(SOURCE_ENV, source)?;
    let helper_bytes = parse_hex::<32>(HELPER_ENV, helper)?;
    emit_test_values(source, helper);
    let generated = format!(
        "const COMPILED_RELEASE_IDENTITY: Option<CompiledReleaseIdentity> = Some(\n\
         CompiledReleaseIdentity {{ source_sha: {}, helper_sha256: {} }});\n",
        array_literal(&source_bytes),
        array_literal(&helper_bytes),
    );
    write_generated(&generated)
}

fn write_unavailable() -> Result<(), String> {
    println!("cargo:rustc-env={COMPILED_SOURCE_ENV}=unavailable");
    println!("cargo:rustc-env={COMPILED_HELPER_ENV}=unavailable");
    write_generated("const COMPILED_RELEASE_IDENTITY: Option<CompiledReleaseIdentity> = None;\n")
}

fn emit_test_values(source: &str, helper: &str) {
    println!("cargo:rustc-env={COMPILED_SOURCE_ENV}={source}");
    println!("cargo:rustc-env={COMPILED_HELPER_ENV}={helper}");
}

fn parse_hex<const LENGTH: usize>(name: &str, value: &str) -> Result<[u8; LENGTH], String> {
    if value.len() != LENGTH * 2 {
        return Err(format!(
            "{name} must contain exactly {} lowercase hexadecimal characters",
            LENGTH * 2
        ));
    }
    let mut decoded = [0_u8; LENGTH];
    for (index, byte) in decoded.iter_mut().enumerate() {
        let offset = index * 2;
        let high = value
            .as_bytes()
            .get(offset)
            .copied()
            .and_then(hex_nibble)
            .ok_or_else(|| format!("{name} must use lowercase hexadecimal"))?;
        let low = value
            .as_bytes()
            .get(offset + 1)
            .copied()
            .and_then(hex_nibble)
            .ok_or_else(|| format!("{name} must use lowercase hexadecimal"))?;
        *byte = high << 4 | low;
    }
    Ok(decoded)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn array_literal(bytes: &[u8]) -> String {
    let values = bytes
        .iter()
        .map(|byte| format!("0x{byte:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

fn write_generated(source: &str) -> Result<(), String> {
    let output = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo did not provide OUT_DIR".to_owned())?;
    fs::write(output.join("compile_identity.rs"), source)
        .map_err(|error| format!("could not write generated identity: {error}"))
}
